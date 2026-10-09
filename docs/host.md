# ホストからの使い方

bitviewを読み込み、値を渡して描画するRustのAPIをまとめます。テンプレートの書き方は[言語](language.md)を参照してください。

## 読み込みと描画

ホストは、テンプレートで使う型に名前を付け、ソースと一緒に `Program::parse` に渡します。

```rust
use bitview::{Html, HtmlType, Program, Source, Type, Value};

let page_type = Type::record([
    ("title", Type::String),
    ("content", Type::Html(HtmlType::Flow)),
]);
let program = Program::parse(
    &[Source {
        name: "views/page.bv",
        text: r#"(defn page [ctx Page] (html (body (h1 ctx.title) ctx.content)))"#,
    }],
    &[("Page", page_type.clone())],
)?;
program.check(&[("page", page_type)])?;

let ctx = Value::record([
    ("title", Value::from("<Hello>")),
    ("content", Value::from(Html::text("Body"))),
]);
let page = program.render("page", ctx)?.to_document()?;
assert_eq!(page, "<!doctype html><html><body><h1>&lt;Hello&gt;</h1>Body</body></html>");
```

- `Program::parse(sources, types)` は、すべての関数を引数の型で1回ずつ検査します（[型の検査](language.md#型の検査)）。
- `Program::check(&[(入口, ctx の型), …])` は、ホストが渡す型の値を入口が受け取れること、つまりその型が入口の引数の型に合い、入口がHtmlを返すことを確かめます。
- `Program::render` は、ホストが渡す値を入口の引数の型に合わせてから本体に渡します。レコードは型に書いたフィールドだけを残し、文字列やリストはHtmlに変えます。型に合わない値は、`Program::check` と同じ文言のエラーになります。
- `Type::validate` は、値がその型にちょうど合うかを確かめます。検査に使った型と実際に渡す値がずれていないことを、これで確かめられます。

## ホストが名前を付ける型

ホストは、テンプレートに渡すデータの形に名前を付け、`Program::parse` の `types` に渡します。名前は大文字で始まり、英字と数字だけからなります。

```rust
let tag = Type::record([("name", Type::String), ("url", Type::String)]);
let page = Type::record([("title", Type::String), ("tags", Type::list(tag.clone()))]);
let program = Program::parse(&sources, &[("Tag", tag), ("TagsPage", page)])?;
```

```clojure
(defn tags [ctx TagsPage]
  (ul (map ctx.tags tag-link)))

(defn- tag-link [tag Tag]
  (li (a {:href tag.url} tag.name)))
```

データを作るホストが型も定義するので、テンプレートとホストで同じ形を二重に書かずに済みます。部品は、ページの型を受け取る代わりに、`[page {:site Site :style Metadata}]` のように自分が読む部分だけを書けば、どのページの値もそのまま受け取れます。

`declare_types(&types)` は、名前の付いた型を、テンプレートで型を書くときと同じ構文で書き出します。ほかの名前付きの型と同じ部分は、その名前で書きます。

```text
Tag {:name String
     :url String}

TagsPage {:title String
          :tags [Tag]}
```

ホストがこれを出力するコマンド（genbitなら `genbit types`）を持てば、[VS Codeの拡張機能](../editors/vscode/README.md)が、型の名前から宣言へ移動したり、ホバーで中身を表示したりできます。

## ホストが作るHtml

- Htmlは構築APIでしか作れません。ホストがRustで組み立てる要素にも、テンプレートと同じ属性・入れ子・URLの規則を当てます。ホストが渡すHtmlの型は `HtmlType` の `Flow`・`Phrasing`・`Metadata` のどれかです。
- `<style>` は `Html::style` で作ります。
- JSONのデータ（JSON-LDなど）は `Html::json` で作ります。受け付けるのは、ブラウザーがスクリプトとして実行しないJSONのMIMEタイプ（`application/json` と `application/…+json`）だけです。

## 呼ばれうる関数とCSSの順序

呼び出し先は読み込み時にすべて決まるので、`Program::functions_used_by` で、ページの入口から呼ばれうる `defn` の関数を、呼ばれる側が先になる順で得られます。genbitはこれを使い、関数ごとのCSSファイル（`views/components/entry-list.css` など）をその関数を使うページにだけ入れます。

`defn-` の関数は、名前がファイルをまたいで一意でないので、この一覧に含めず、CSSファイルも対応させません。`defn-` の関数を通って呼ばれる `defn` の関数は一覧に含みます。`defn-` の関数が出す要素のスタイルは、同じファイルの `defn` の関数のCSSに書きます（`entry-list.bv` の `defn-` の `entry-item` が出す `li` は `entry-list.css` に書く）。

この順でCSSを連結すると、骨組みの関数の規則を部品が、部品の規則をそれを使う側が上書きできます。互いに呼び合わない関数どうしは、ソースで呼んでいる順に並びます。兄弟の部品が同じ要素を指定し合うと、呼び出しの順を変えただけで見た目が変わるので、部品のCSSは自分のクラスの中だけを指定します（`entry-list.css` なら `.entry-list` の中）。
