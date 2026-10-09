// Every function is defined as `(defn name [params] ...)` or, private to its file, as
// `(defn- name [params] ...)`. Public functions share one namespace across the .bv files of a
// program, and a private one hides a public one of the same name within its file. So regular
// expressions over the code find definitions and references without a parser.

const NAME = "[a-z][a-z0-9]*(?:-[a-z0-9]+)*";
const DEFINITION = new RegExp(`\\(\\s*defn(-?)\\s+(${NAME})\\s*\\[`, "g");
// A parameter name, not a record key after `:` or the lowercase part of a type such as `String`.
const PARAM = new RegExp(`(?<![A-Za-z0-9:.-])${NAME}`, "y");

/** `text` with strings and comments replaced by spaces, so that every offset stays the same. */
function codeOnly(text) {
  let code = "";
  let i = 0;
  const blank = (character) => (character === "\n" ? "\n" : " ");
  while (i < text.length) {
    if (text[i] === '"') {
      code += " ";
      i += 1;
      while (i < text.length && text[i] !== '"') {
        const length = text[i] === "\\" ? 2 : 1;
        for (const character of text.slice(i, i + length)) code += blank(character);
        i += length;
      }
      if (i < text.length) {
        code += " ";
        i += 1;
      }
    } else if (text[i] === ";") {
      while (i < text.length && text[i] !== "\n") {
        code += " ";
        i += 1;
      }
    } else {
      code += text[i];
      i += 1;
    }
  }
  return code;
}

function definitionsIn(text) {
  const found = [];
  const code = codeOnly(text);
  for (const match of code.matchAll(DEFINITION)) {
    const [, hyphen, name] = match;
    const nameOffset = match.index + match[0].indexOf(name, match[0].indexOf("defn") + 4);
    const params = paramsAt(code, match.index + match[0].length);
    found.push({ name, offset: nameOffset, start: match.index, params, private: hyphen === "-" });
  }
  return found;
}

/**
 * The parameter names in the list that starts at `offset`, just after its `[`. Types such as
 * `[String]` and `{:draft Bool}` follow the names, so only names outside them count.
 */
function paramsAt(code, offset) {
  const params = [];
  let depth = 0;
  for (let i = offset; i < code.length; i += 1) {
    const character = code[i];
    if (character === "[" || character === "{") depth += 1;
    else if (character === "}") depth -= 1;
    else if (character === "]") {
      if (depth === 0) break;
      depth -= 1;
    } else if (depth === 0) {
      PARAM.lastIndex = i;
      const param = PARAM.exec(code);
      if (param) {
        params.push({ name: param[0], offset: i });
        i += param[0].length - 1;
      }
    }
  }
  return params;
}

/** A parameter hides functions of the same name, so the enclosing function's parameters come first. */
function symbolAt(text, definitions, wordStart, word) {
  // A field after `.`, a record key after `:`, and the rest of a type such as `String` are not names.
  if (/[.:A-Za-z0-9]/.test(text[wordStart - 1] ?? "")) return null;
  const enclosing = definitions.filter((definition) => definition.start <= wordStart).pop();
  const param = enclosing?.params.find((candidate) => candidate.name === word);
  if (param) return { kind: "param", scope: enclosing.start, declaration: param.offset };
  return { kind: "function", name: word };
}

function occurrencesIn(text, symbol) {
  const name = symbol.kind === "function" ? symbol.name : wordOf(text, symbol.declaration);
  const definitions = definitionsIn(text);
  const code = codeOnly(text);
  const found = [];
  for (const match of code.matchAll(new RegExp(`(?<![a-z0-9-])${name}(?![a-z0-9-])`, "g"))) {
    const at = symbolAt(code, definitions, match.index, name);
    if (at?.kind !== symbol.kind || (symbol.kind === "param" && at.scope !== symbol.scope)) continue;
    found.push({ offset: match.index, length: name.length });
  }
  return found;
}

/**
 * The documents where the name of the function `name` in `document` means the same function, each
 * with its definitions of it: `document` alone if it defines `name` with `defn-`, and otherwise
 * every document that does not keep a private `name` of its own.
 */
function functionScope(document, name, documents) {
  const named = (candidate, isPrivate) =>
    definitionsIn(candidate.getText()).filter(
      (definition) => definition.name === name && definition.private === isPrivate,
    );
  const own = named(document, true);
  if (own.length > 0) return [{ document, definitions: own }];
  return documents
    .filter((candidate) => named(candidate, true).length === 0)
    .map((candidate) => ({ document: candidate, definitions: named(candidate, false) }));
}

/** What the built-in types are, for hovers. The host's types are declared by its types command. */
const BUILT_IN_TYPES = {
  String: "Text.",
  Bool: "`true` or `false`.",
  Flow: "Html that goes in a `<body>`, such as paragraphs, lists, and links.",
  Phrasing: "Html that goes in a line of text, such as `<span>`, `<a>`, and `<em>`.",
  Metadata: "Html that goes in a `<head>`, such as `<meta>`, styles, and JSON.",
};

/** The type name at `offset`, such as `Entry`, outside strings and comments. */
function typeAt(text, offset) {
  const code = codeOnly(text);
  let start = offset;
  while (start > 0 && /[A-Za-z0-9]/.test(code[start - 1])) start -= 1;
  const name = code.slice(start).match(/^[A-Z][A-Za-z0-9]*/)?.[0];
  if (!name || start + name.length < offset) return null;
  return { name, start };
}

/**
 * The declaration of the type `name` in the output of the types command, which declares one type
 * per paragraph starting with its name: the line it starts on, and its text.
 */
function declarationIn(text, name) {
  const lines = text.split("\n");
  const line = lines.findIndex((candidate) => new RegExp(`^${name}(?![A-Za-z0-9])`).test(candidate));
  if (line < 0) return null;
  let end = line;
  while (end + 1 < lines.length && lines[end + 1].trim() !== "") end += 1;
  return { line, text: lines.slice(line, end + 1).join("\n") };
}

function wordOf(text, offset) {
  return text.slice(offset).match(new RegExp(`^${NAME}`))?.[0];
}

function lookup(text, offset) {
  const code = codeOnly(text);
  if (!/[a-z0-9-]/.test(code[offset] ?? "") && !/[a-z0-9-]/.test(code[offset - 1] ?? "")) return null;
  let start = offset;
  while (start > 0 && /[a-z0-9-]/.test(code[start - 1])) start -= 1;
  const word = wordOf(code, start);
  if (!word || start + word.length < offset) return null;
  const symbol = symbolAt(code, definitionsIn(text), start, word);
  return symbol && { word, start, symbol };
}

function activate(context) {
  const vscode = require("vscode");
  const selector = { language: "bitview" };

  /** The directory that holds views/, where genbit runs. Outside views/, the file's directory. */
  function projectRoot(uri) {
    const parts = uri.path.split("/");
    const views = parts.lastIndexOf("views", parts.length - 2);
    return uri.with({ path: parts.slice(0, views >= 0 ? views : parts.length - 1).join("/") });
  }

  // The host's types are declared by a command, such as `genbit types`, run in the project. Its
  // output is shown as a read-only document, so it never drifts from the host as a file could.
  const TYPES_SCHEME = "bitview-types";
  const typesChanged = new vscode.EventEmitter();

  function typesUri(root) {
    return vscode.Uri.from({ scheme: TYPES_SCHEME, path: `${root.path}/types.bv` });
  }

  function runTypes(root) {
    const command = vscode.workspace.getConfiguration("bitview").get("typesCommand");
    return new Promise((resolve) => {
      if (!command || !vscode.workspace.isTrusted) {
        resolve({ error: "the types command runs only in a trusted workspace with bitview.typesCommand set" });
        return;
      }
      require("child_process").exec(command, { cwd: root.fsPath, timeout: 10000 }, (error, stdout, stderr) =>
        resolve(error ? { error: `${command} failed in ${root.fsPath}: ${stderr || error.message}` } : { text: stdout }),
      );
    });
  }

  /** The declaration of the host type `name` for `document`, and where it is. */
  async function hostType(document, name) {
    const root = projectRoot(document.uri);
    const result = await runTypes(root);
    if (result.error) {
      vscode.window.setStatusBarMessage(`bitview: ${result.error}`, 5000);
      return null;
    }
    typesChanged.fire(typesUri(root));
    const declaration = declarationIn(result.text, name);
    return declaration && { ...declaration, uri: typesUri(root) };
  }

  /** genbit reads every .bv under views/ as one program. Outside views/, a directory is one. */
  function programRoot(uri) {
    const parts = uri.path.split("/");
    const views = parts.lastIndexOf("views", parts.length - 2);
    return uri.with({ path: parts.slice(0, views >= 0 ? views + 1 : parts.length - 1).join("/") });
  }

  async function bitviewFiles(directory) {
    const found = [];
    for (const [name, type] of await vscode.workspace.fs.readDirectory(directory)) {
      const uri = vscode.Uri.joinPath(directory, name);
      if (name.startsWith(".")) continue;
      if (type === vscode.FileType.Directory) found.push(...(await bitviewFiles(uri)));
      else if (type === vscode.FileType.File && name.endsWith(".bv")) found.push(uri);
    }
    return found;
  }

  async function programDocuments(document) {
    let uris;
    try {
      uris = await bitviewFiles(programRoot(document.uri));
    } catch {
      // An unsaved document has no directory.
      return [document];
    }
    return Promise.all(uris.map((uri) => vscode.workspace.openTextDocument(uri)));
  }

  function location(document, offset, length) {
    const start = document.positionAt(offset);
    return new vscode.Location(document.uri, new vscode.Range(start, document.positionAt(offset + length)));
  }

  async function definitions(documents) {
    return documents.flatMap((document) =>
      definitionsIn(document.getText()).map((definition) => ({ ...definition, document })),
    );
  }

  context.subscriptions.push(
    vscode.languages.registerDefinitionProvider(selector, {
      // On a definition itself this returns that definition, and VS Code then shows its references.
      async provideDefinition(document, position) {
        const type = typeAt(document.getText(), document.offsetAt(position));
        if (type) {
          if (type.name in BUILT_IN_TYPES) return null;
          const declared = await hostType(document, type.name);
          return declared && new vscode.Location(declared.uri, new vscode.Range(declared.line, 0, declared.line, type.name.length));
        }
        const found = lookup(document.getText(), document.offsetAt(position));
        if (!found) return null;
        const { symbol, word } = found;
        if (symbol.kind === "param") return location(document, symbol.declaration, word.length);
        return functionScope(document, symbol.name, await programDocuments(document)).flatMap((scope) =>
          scope.definitions.map((definition) => location(scope.document, definition.offset, word.length)),
        );
      },
    }),
    vscode.workspace.registerTextDocumentContentProvider(TYPES_SCHEME, {
      onDidChange: typesChanged.event,
      async provideTextDocumentContent(uri) {
        const root = uri.with({ scheme: "file", path: uri.path.slice(0, uri.path.lastIndexOf("/")) });
        const result = await runTypes(root);
        return result.error ? `; ${result.error.replaceAll("\n", "\n; ")}` : result.text;
      },
    }),
    vscode.languages.registerHoverProvider(selector, {
      async provideHover(document, position) {
        const type = typeAt(document.getText(), document.offsetAt(position));
        if (!type) return null;
        const range = new vscode.Range(
          document.positionAt(type.start),
          document.positionAt(type.start + type.name.length),
        );
        if (type.name in BUILT_IN_TYPES) {
          return new vscode.Hover(new vscode.MarkdownString(`**${type.name}**: ${BUILT_IN_TYPES[type.name]}`), range);
        }
        const declared = await hostType(document, type.name);
        if (!declared) return null;
        return new vscode.Hover(new vscode.MarkdownString().appendCodeblock(declared.text, "bitview"), range);
      },
    }),
    vscode.languages.registerReferenceProvider(selector, {
      async provideReferences(document, position, referenceContext) {
        const found = lookup(document.getText(), document.offsetAt(position));
        if (!found) return null;
        const { symbol } = found;
        const scopes =
          symbol.kind === "param"
            ? [{ document, definitions: [{ offset: symbol.declaration }] }]
            : functionScope(document, symbol.name, await programDocuments(document));
        return scopes.flatMap(({ document: candidate, definitions: declared }) => {
          const declarations = new Set(declared.map((definition) => definition.offset));
          return occurrencesIn(candidate.getText(), symbol)
            .filter((occurrence) => referenceContext.includeDeclaration || !declarations.has(occurrence.offset))
            .map((occurrence) => location(candidate, occurrence.offset, occurrence.length));
        });
      },
    }),
    vscode.languages.registerDocumentSymbolProvider(selector, {
      provideDocumentSymbols(document) {
        return definitionsIn(document.getText()).map((definition) => {
          const range = location(document, definition.offset, definition.name.length).range;
          const detail = `[${definition.params.map((param) => param.name).join(" ")}]`;
          return new vscode.DocumentSymbol(definition.name, detail, vscode.SymbolKind.Function, range, range);
        });
      },
    }),
    vscode.languages.registerWorkspaceSymbolProvider({
      async provideWorkspaceSymbols(query) {
        const uris = await vscode.workspace.findFiles("**/*.bv", "**/{node_modules,dist,target}/**");
        const documents = await Promise.all(uris.map((uri) => vscode.workspace.openTextDocument(uri)));
        return (await definitions(documents))
          .filter((definition) => definition.name.includes(query))
          .map(
            (definition) =>
              new vscode.SymbolInformation(
                definition.name,
                vscode.SymbolKind.Function,
                "",
                location(definition.document, definition.offset, definition.name.length),
              ),
          );
      },
    }),
  );
}

function deactivate() {}

module.exports = {
  activate,
  deactivate,
  codeOnly,
  declarationIn,
  definitionsIn,
  functionScope,
  lookup,
  occurrencesIn,
  typeAt,
};
