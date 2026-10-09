# bitview

HTMLを文字列ではなく値として組み立てる、小さい純粋関数型のテンプレート言語です。描画の前にテンプレートを型で検査し、エスケープを外す手段やスクリプトを書く手段を持ちません。

```clojure
(defn entry [ctx EntryPage]
  (html {:lang "ja"}
    (head (title (concat ctx.entry.title " | " ctx.site.title)))
    (body
      (article
        (h1 ctx.entry.title)
        (ul (map ctx.entry.tags tag-item))
        ctx.content))))

(defn- tag-item [tag {:url String :name String}]
  (li (a {:href tag.url} tag.name)))
```

`EntryPage` は、ホストがRustで定義して名前を付けた型です。

> [!NOTE]
> bitviewは、静的サイトジェネレーター[genbit](https://github.com/nnenn0/genbit)のテンプレートを書くために作っている個人用の言語で、genbitからRustのクレートとして使います。構文、標準の要素と属性、RustのAPIは、どの版でも互換性なく変わる可能性があります。使う場合は版のタグを固定してください。

## なぜbitviewか

多くのテンプレートエンジンは、HTMLの断片を文字列としてつなぎます。自動でエスケープするエンジンでも、`safe` のような指定で外せます。存在しない変数を空文字として出力するエンジンもあり、タグの入れ子や属性の綴りの誤りは、出力を見るまでわかりません。bitviewはHTMLを値として扱い、こうした誤りを描画の前に止めます。

### 値と関数だけの言語

値はString、Bool、List、Record、Htmlの5種類で、数値はありません。HTMLの要素は関数として呼び、部品は関数として定義します。

再帰は読み込み時に拒否するので、評価は必ず停止します。テンプレートからはファイルやネットワークを読めず、使えるのはホストから渡された値だけなので、同じ値を渡せば常に同じHTMLができます。

### エスケープとURLの検査

文字列は、HTMLに書き出すときに必ずエスケープします。文字列をHTMLに変える手段（`raw`、`safe`）はなく、スクリプトを実行する `script` 要素、`on*` 属性、`style` 属性は、テンプレートからもホストのRustからも作れません。URLを取る属性に書けるスキームは `http`・`https`・`mailto` と相対URLだけです。

### 描画の前の型の検査

関数の引数には必ず型を書きます。読み込むときに、すべての関数を引数の型だけを手がかりに検査するので、特定のデータのときにしか通らない分岐の誤りも、描画の前に、誤りのある関数の位置で見つかります。

```text
views/components/draft-badge.bv:2:54: unknown field "titel" (fields: title, draft)
  in draft-badge
```

存在しないフィールドのほか、型の合わない値、要素に書けない属性（`herf` のような綴りの誤り）、`(p (div ...))` のようにHTMLで置けない入れ子も見つかります（[型の検査](docs/language.md#型の検査)）。

## bitviewがやらないこと

テンプレートは、ホストが整形した値をHTMLに配置するだけにとどめます。そのため、次の機能は持ちません。

- 数値、算術、比較の演算子。日付や件数の表示は、ホストが整形した文字列を渡します。
- 再帰とループ。繰り返しは `map` だけで書きます。
- ファイルの読み込み、インクルード、テンプレートの継承。共通の部分は関数にして呼びます。
- 無名関数と、関数を値として渡すこと。`map` の2つ目の引数にだけ、定義した関数の名前を書けます。
- HTMLの文字列をそのまま出すこと、スクリプト、インラインのスタイル。CSSはホストが `Html::style` で渡します。

新しい構文、値の型、標準関数を足す前に、genbitの実際のテンプレートを書くために必要かを確かめます。genbitが整形済みの値を渡すことで済むなら、言語には足しません。

## 使い方

genbitと同じく、Gitの依存として版のタグで参照します。`tag` には[タグの一覧](https://github.com/nnenn0/bitview/tags)にある版を指定してください。

```toml
[dependencies]
bitview = { git = "https://github.com/nnenn0/bitview", tag = "vX.Y.Z" }
```

ホストは、テンプレートで使う型に名前を付け、ソースと一緒に `Program::parse` に渡して検査し、値を渡して描画します（[ホストからの使い方](docs/host.md)）。

VS Codeでは、`editors/vscode` の拡張機能でハイライト、定義へ移動、参照の検索ができます（[editors/vscode/README.md](editors/vscode/README.md)）。

## ドキュメント

| ドキュメント | 内容 |
| --- | --- |
| [言語](docs/language.md) | 構文、関数、標準ライブラリ、型と型の検査、HTMLの安全性 |
| [ホストからの使い方](docs/host.md) | RustのAPI、ホストが名前を付ける型、ホストが作るHtml、関数ごとのCSSの順序 |

## 開発

Rustの検証は `compose.yaml` の `cli` サービスで行います。

```sh
docker compose build cli
docker compose run --rm cli fmt --check
docker compose run --rm cli clippy --locked --all-targets --all-features -- -D warnings
docker compose run --rm cli test --locked --all-targets --all-features
```

## ライセンス

[MIT License](LICENSE)です。
