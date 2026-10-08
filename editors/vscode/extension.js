// Every function is defined as `fn name(params) =>`, and the .bv files of a program share one
// namespace. So regular expressions over the code find definitions and references without a parser.

const NAME = "[a-z][a-z0-9]*(?:-[a-z0-9]+)*";
const DEFINITION = new RegExp(`(?<![a-z0-9.-])fn\\s+(${NAME})\\s*\\(([^)]*)\\)`, "g");
const PARAM = new RegExp(NAME, "g");

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
    } else if (text[i] === "-" && text[i + 1] === "-") {
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
  for (const match of codeOnly(text).matchAll(DEFINITION)) {
    const nameOffset = match.index + match[0].indexOf(match[1], 2);
    const paramsOffset = match.index + match[0].indexOf("(") + 1;
    const params = [...match[2].matchAll(PARAM)].map((param) => ({
      name: param[0],
      offset: paramsOffset + param.index,
    }));
    found.push({ name: match[1], offset: nameOffset, start: match.index, params });
  }
  return found;
}

/** A parameter hides functions of the same name, so the enclosing function's parameters come first. */
function symbolAt(text, definitions, wordStart, word) {
  if (text[wordStart - 1] === ".") return null;
  if (/^\s*:/.test(text.slice(wordStart + word.length))) return null;
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
        const found = lookup(document.getText(), document.offsetAt(position));
        if (!found) return null;
        const { symbol, word } = found;
        if (symbol.kind === "param") return location(document, symbol.declaration, word.length);
        return (await definitions(await programDocuments(document)))
          .filter((definition) => definition.name === symbol.name)
          .map((definition) => location(definition.document, definition.offset, definition.name.length));
      },
    }),
    vscode.languages.registerReferenceProvider(selector, {
      async provideReferences(document, position, referenceContext) {
        const found = lookup(document.getText(), document.offsetAt(position));
        if (!found) return null;
        const { symbol } = found;
        const documents = symbol.kind === "param" ? [document] : await programDocuments(document);
        return documents.flatMap((candidate) => {
          const text = candidate.getText();
          const declarations = new Set(
            symbol.kind === "param"
              ? [symbol.declaration]
              : definitionsIn(text)
                  .filter((definition) => definition.name === symbol.name)
                  .map((definition) => definition.offset),
          );
          return occurrencesIn(text, symbol)
            .filter((occurrence) => referenceContext.includeDeclaration || !declarations.has(occurrence.offset))
            .map((occurrence) => location(candidate, occurrence.offset, occurrence.length));
        });
      },
    }),
    vscode.languages.registerDocumentSymbolProvider(selector, {
      provideDocumentSymbols(document) {
        return definitionsIn(document.getText()).map((definition) => {
          const range = location(document, definition.offset, definition.name.length).range;
          const detail = `(${definition.params.map((param) => param.name).join(", ")})`;
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

module.exports = { activate, deactivate, codeOnly, definitionsIn, lookup, occurrencesIn };
