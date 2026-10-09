# bitview

HTML を文字列ではなく値として組み立てる、小さい純粋関数型のテンプレート言語。描画の前にテンプレートを型で検査し、エスケープの漏れやスクリプトを書く手段を持たない。

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

`EntryPage` は、ホストが Rust で定義して名前を付けた型である。

> [!NOTE]
> bitviewは、静的サイトジェネレーター [genbit](https://github.com/nnenn0/genbit) のテンプレートを書くために作っている個人用の言語で、genbit から Rust のクレートとして使う。構文、標準の要素と属性、Rust の API は、どの版でも互換性なく変わる可能性がある。使う場合は版のタグを固定すること。

## なぜbitviewか

多くのテンプレートエンジンは、HTML の断片を文字列としてつなぐ。自動でエスケープするエンジンでも、`safe` のような指定で外せる。存在しない変数を空文字として出力するエンジンもあり、タグの入れ子や属性の綴りの誤りは、出力を見るまでわからない。bitview は HTML を値として扱い、こうした誤りを描画の前に止める。

### 小さい

値は String、Bool、List、Record、Html の5種類で、数値はない。HTML の要素は関数として呼び、部品は関数として定義する。再帰は読み込み時に拒否し、ファイルやネットワークにもアクセスしないので、評価は必ず停止し、同じ値からは常に同じ HTML ができる。

### 安全

文字列は、HTML に書き出すときに必ずエスケープする。文字列を HTML に変える手段（`raw`、`safe`）はなく、スクリプトを実行する `script` 要素、`on*` 属性、`style` 属性はテンプレートからもホストの Rust からも作れない。URL を取る属性に書けるスキームは `http`・`https`・`mailto` と相対 URL だけである。

### 描画の前に誤りが見つかる

関数の引数には必ず型を書く。読み込むときに、すべての関数を、引数の型だけを手がかりに1つずつ検査する。検査は `if` の両側と `map` の中にも及ぶので、特定のデータのときにしか通らない分岐の誤りも、描画の前に、誤りのある関数の位置で見つかる。

```text
views/components/badge.bv:2:30: unknown field "titel" (fields: title, draft)
  in badge
```

見つかる誤りは、存在しない項目、型の合わない値、引数の型に合わない呼び出し、要素に書けない属性（`herf` のような綴りの誤りを含む）、`(p (div ...))` のように HTML で置けない入れ子である。

```text
views/page.bv:2:6: <p> cannot contain block elements such as <p> and <div>; it takes text, phrasing elements such as <span> and <a>
  in page
```

## bitviewがやらないこと

テンプレートは、ホストが整形した値を HTML に配置するだけにとどめる。そのため、次の機能は持たない。

- 数値、算術、比較の演算子。日付や件数の表示は、ホストが整形した文字列を渡す。
- 再帰とループ。繰り返しは `map` だけで書く。
- ファイルの読み込み、インクルード、テンプレートの継承。共通の部分は関数にして呼ぶ。
- 無名関数と、関数を値として渡すこと。`map` の2つ目の引数にだけ、定義した関数の名前を書ける。
- HTML の文字列をそのまま出すこと、スクリプト、インラインのスタイル。CSS はホストが `Html::style` で渡す。

新しい構文、値の型、標準関数を足す前に、genbit の実際のテンプレートを書くために必要かを確かめる。genbit が整形済みの値を渡すことで済むなら、言語には足さない。

## 使い方

genbit と同じく、Git の依存として版のタグで参照する。

```toml
[dependencies]
bitview = { git = "https://github.com/nnenn0/bitview", tag = "v0.3.0" }
```

ホストは、テンプレートで使う型に名前を付け、ソースと一緒に `Program::parse` に渡す。`Program::parse` は、すべての関数をその時点で検査する。ホストは、入口の関数が自分の渡す型の値を受け取れることを `Program::check` で確かめてから、値を渡して描画する。

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

VS Code では、`editors/vscode` の拡張機能でハイライト、定義へ移動、参照の検索ができる（[editors/vscode/README.md](editors/vscode/README.md)）。

## 言語

- 値: String、Bool、List、Record、Html。
- 型: `String`、`Bool`、Html の `Flow`・`Phrasing`・`Metadata`、ホストが名前を付けた型（`Entry` など）、リスト `[String]`、レコード `{:title String :draft Bool}`。大文字で始まる語は型の名前で、引数の後にだけ書ける。
- 名前: 小文字と数字を `-` でつないだもの（`entry-list`）。CSS のクラス名やファイル名と同じ綴りにできる。`defn`・`defn-`・`if` は名前に使えない。
- コメント: `;` から行末まで。
- 構文: Clojure に似た S 式で書く。

  | 書き方 | 意味 |
  | --- | --- |
  | `(defn name [a String b Entry] expr)` | 関数の定義。引数ごとに名前の後に型を書く。本体は1つの式 |
  | `(defn- name [a String] expr)` | 定義したファイルの中でだけ呼べる関数の定義 |
  | `(f x y)` | 関数の呼び出し。先頭に書けるのは関数の名前だけ |
  | `a.b.c` | 引数 `a` のフィールドの参照。`.` の前後に空白を入れない |
  | `"text"` | 文字列 |
  | `[x y z]` | リスト |
  | `{:href "/" :class "x"}` | レコード。キーは `:` に続く名前 |
  | `(if c x y)` | `c` が真なら `x`、偽なら `y` |

  フィールドを読めるのは引数の名前だけで、`(f x).b` とは書けない。関数が返したレコードは、それを受け取る関数の引数として読む。
- 標準ライブラリ: HTML 要素関数（`(p ...)`、`(a {:href "/"} "top")` など。最初の引数がレコードなら属性）、`concat`（文字列の連結）、`(map list 関数名)`。
- ソースファイルの拡張子は `.bv` とする。

`defn` で定義した関数は、すべてのファイルとホストから名前で呼べるので、名前はファイルをまたいで1つずつしか定義できない。`defn-` で定義した関数は、そのファイルの中からしか呼べず、ホストも入口として呼べない。そのため、別々のファイルが同じ名前の `defn-` の関数を持てる。ファイルの中では、`defn-` の関数が、ほかのファイルにある同じ名前の `defn` の関数を隠すので、ほかのファイルに関数が増えても、そのファイルの呼び出し先は変わらない。1つのファイルの中で同じ名前を2回定義するとエラーになる。

空要素（`br`、`img` など）は子の引数を取らない。引数は同じ名前の関数を隠す（`(defn heading [title String] (h1 title))` と書ける）。隠された名前を呼ぶとエラーになる。

## 型の検査

`Program::parse(sources, types)` は、すべての関数を、引数の型で1回ずつ検査する。関数の本体は、呼び出し元から何が渡されるかに関係なく、型だけで検査が決まるので、どこからも呼ばれない関数も検査される。誤りは、誤りのある関数の中の位置で報告する。

`Program::check(&[(入口, ctx の型), …])` は、ホストが渡す型の値を入口が受け取れること、つまりその型が入口の引数の型に合い、入口が Html を返すことを確かめる。

### 引数の型

```clojure
(defn draft-badge [entry {:draft Bool}]
  (if entry.draft (span {:class "draft-badge"} "draft") []))
```

- レコードの型は、関数が読むフィールドを並べたもので、それ以上のフィールドを持つレコードも渡せる。記事と一覧の項目のように、別々の型のレコードを同じ関数に渡せる。
- 関数の中から見えるのは、型に書いたフィールドだけである。書いていないフィールドを読むとエラーになるので、型を見れば、関数が何を読むかがわかる。
- 呼び出す側では、渡す値が型に合うかを検査する。合わなければ、呼び出しの位置でエラーになる（`entry has no field "draft", which its type lists`）。
- 本体は、渡される値ではなく書いた型で検査する。`[x Flow]` の `x` は、ブロックを含むかもしれない Html として扱うので、`(p x)` は文字列しか渡されなくてもエラーになる。文中に置く Html なら `Phrasing` と書く。
- 描画では、ホストが渡す値を入口の引数の型に合わせてから本体に渡す。レコードは型に書いたフィールドだけを残し、文字列やリストは Html に変える。型に合わない値は、`Program::check` と同じ文言のエラーになる。
- Html の型は、`Flow`・`Phrasing`・`Metadata` の3つである。`li` だけを並べた Html のように、これらで表せない Html を受け取る関数は書けない。genbit で必要になったときに、型の名前を足す。

### ホストが名前を付ける型

ホストは、テンプレートに渡すデータの形に名前を付け、`Program::parse` の `types` に渡す。名前は大文字で始まり、英字と数字だけからなる。

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

`declare_types(&types)` は、名前の付いた型を、テンプレートで型を書くときと同じ構文で書き出す。ほかの名前付きの型と同じ部分は、その名前で書く。

```text
Tag {:name String
     :url String}

TagsPage {:title String
          :tags [Tag]}
```

ホストがこれを出力するコマンド（genbit なら `genbit types`）を持てば、VS Code の拡張機能が、型の名前から宣言へ移動したり、ホバーで中身を表示したりできる。

データを作るホストが型も定義するので、テンプレートとホストで同じ形を二重に書かずに済む。部品は、ページの型を受け取る代わりに、`[page {:site Site :style Metadata}]` のように自分が読む部分だけを書けば、どのページの値もそのまま受け取れる。

### 値と Html

- Html は HTML の断片、つまりノードの並びである。文字列は1つのテキストノードの断片として、断片のリストはそれらを順に並べた1つの断片として、Html の代わりに使える。`[]` は空の断片でもあるので、何も出さない側は `[]` と書く。
- `if` の型とリストの要素の型は、両側を合わせられる最小の型になる。`(if c (span "x") [])` は Html、`(if c [(h1 "x") (p "y")] [])` は Html のリストになり、どちらも断片として要素の子に置ける。
- Html の型は、置ける場所の分類（文中に置ける要素、ブロック、`li`、表の行、`head` の中身など）を持つ。要素ごとに中に置ける分類が決まっていて、`(p (div ...))`・`(ul (p ...))`・`(head (p ...))`・`a` の中の `a` は、ブラウザーが黙って組み替える前に、型の検査でエラーになる。`a` は中身と同じ分類になるので、ブロックを包んだ `a` はブロックが置ける場所にだけ置ける。ホストが渡す Html の型は `HtmlType` の Flow（`body` の中身）、Phrasing（文中に置けるもの）、Metadata（`head` の中身）のどれかで、ホストが Rust で組み立てる要素にも同じ規則を当てる。
- URL のスキームなど、値そのものに依存する検査は描画時に行う。
- `Type::validate` は、値がその型にちょうど合うかを確かめる。ホストは、検査に使った型と実際に渡す値がずれていないことを、これで確かめられる。

## HTML の安全性

- String は、シリアライザーが必ずエスケープする。Html は構築 API でしか作れない。
- 書ける属性は、全要素に共通の属性（`id`・`class`・`title`・`lang` など）、要素ごとに決めた属性（`a` の `href`・`target`・`rel` など）、`data-*`、`aria-*` だけ。
- `href`・`src`・`cite` と、複数の URL を並べる `srcset`・`imagesrcset`・`ping` は、どの URL もスキームが `http`・`https`・`mailto` か相対 URL でなければエラーにする。
- `<style>` と JSON のデータ（JSON-LD など）だけは、ホストが `Html::style`・`Html::json` で作って渡す。`Html::json` が受け付けるのは、ブラウザーがスクリプトとして実行しない JSON の MIME タイプ（`application/json` と `application/…+json`）だけ。

## 呼ばれうる関数と CSS の順序

呼び出し先は読み込み時にすべて決まるので、`Program::functions_used_by` で、ページの入口から呼ばれうる `defn` の関数を、呼ばれる側が先になる順で得られる。genbit はこれを使い、関数ごとの CSS ファイル（`views/components/entry-list.css` など）をその関数を使うページにだけ入れる。

`defn-` の関数は、名前がファイルをまたいで一意でないので、この一覧に含めず、CSS ファイルも対応させない。`defn-` の関数を通って呼ばれる `defn` の関数は一覧に含む。`defn-` の関数が出す要素のスタイルは、同じファイルの `defn` の関数の CSS に書く（`entry-list.bv` の `defn-` の `entry-item` が出す `li` は `entry-list.css` に書く）。

この順で CSS を連結すると、骨組みの関数の規則を部品が、部品の規則をそれを使う側が上書きできる。互いに呼び合わない関数どうしは、ソースで呼んでいる順に並ぶ。兄弟の部品が同じ要素を指定し合うと、呼び出しの順を変えただけで見た目が変わるので、部品の CSS は自分のクラスの中だけを指定する（`entry-list.css` なら `.entry-list` の中）。

## 開発

Rust の検証は `compose.yaml` の `cli` サービスで行う。

```sh
docker compose build cli
docker compose run --rm cli fmt --check
docker compose run --rm cli clippy --locked --all-targets --all-features -- -D warnings
docker compose run --rm cli test --locked --all-targets --all-features
```

## ライセンス

[MIT License](LICENSE)。
