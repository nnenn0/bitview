# bitview

HTML を、少数の値と関数だけで組み立てる小さい純粋関数型テンプレート言語。静的サイトジェネレーター [genbit](https://github.com/nnenn0/genbit) のテンプレートを書くために作っており、genbit から Rust のクレートとして使う。ソースファイルの拡張子は `.bv` とする。

```
fn page(ctx) =>
  html({lang: "ja"},
    head(title(concat(ctx.article.title, " | ", ctx.site.title))),
    body(
      article(
        h1(ctx.article.title),
        ul(map(ctx.article.tags, tag-item)),
        ctx.content
      )
    )
  )

fn tag-item(tag) => li(a({href: tag.url}, tag.name))
```

## 言語

- 値: String、Bool、List、Record、Html。
- 名前: 小文字と数字を `-` でつないだもの（`entry-list`）。CSS のクラス名やファイル名と同じ綴りにできる。
- コメント: `--` から行末まで。
- 構文: 関数定義 `fn name(params) => expr`、関数呼び出し、引数、フィールド参照 `a.b`、文字列・リスト・レコードのリテラル、`if c then x else y`。
- 標準ライブラリ: HTML 要素関数（`p(...)`、`a({href: "/"}, "top")` など。最初の引数がレコードなら属性）、`concat`（文字列の連結）、`map(list, 関数名)`。

関数は値ではなく、`map` の第2引数にだけトップレベル関数の名前を書ける。空要素（`br`、`img` など）は子の引数を取らない。引数は同じ名前の関数を隠す（`fn heading(title) => h1(title)` と書ける）。隠された名前を呼ぶとエラーになる。再帰は読み込み時に拒否するので、評価は必ず停止する。ファイルやネットワークへのアクセスはない。

呼び出し先は読み込み時にすべて決まるので、`Program::functions_used_by` で、ページの入口から呼ばれうる関数を、呼ばれる側が先になる順で得られる。genbit はこれを使い、関数ごとの CSS ファイル（`views/components/entry-list.css` など）をその関数を使うページにだけ入れる。

この順で CSS を連結すると、骨組みの関数の規則を部品が、部品の規則をそれを使う側が上書きできる。互いに呼び合わない関数どうしは、ソースで呼んでいる順に並ぶ。兄弟の部品が同じ要素を指定し合うと、呼び出しの順を変えただけで見た目が変わるので、部品の CSS は自分のクラスの中だけを指定する（`entry-list.css` なら `.entry-list` の中）。

## 型の検査

`Program::check(&[(入口, ctx の型), …])` は、描画の前に、それぞれの入口をその型のどの値で描画しても型や項目の誤りが起きないことを確かめる。テンプレートに型を書く必要はない。関数は入口からの呼び出しを通して検査するので、どの入口からも呼ばれない関数はエラーにする。

- `if` の両側と、`map` が適用する関数の中も検査する。描画では特定のデータのときにしか通らない部分の誤り（項目名の打ち間違いなど）も、読み込み時に見つかる。
- 型は String、Bool、Html、List、Record。Html は HTML の断片、つまりノードの並びである。文字列は1つのテキストノードの断片として、断片のリストはそれらを順に並べた1つの断片として、Html の代わりに使える。`[]` は空の断片でもあるので、何も出さない側は `else []` と書く。
- `if` の型とリストの要素の型は、両側を合わせられる最小の型になる。`if c then span("x") else []` は Html、`if c then [h1("x"), p("y")] else []` は Html のリストになり、どちらも断片として要素の子に置ける。
- Html の型は、置ける場所の分類（文中に置ける要素、ブロック、`li`、表の行、`head` の中身など）を持つ。要素ごとに中に置ける分類が決まっていて、`p(div(...))`・`ul(p(...))`・`head(p(...))`・`a` の中の `a` は、ブラウザーが黙って組み替える前に、型の検査でエラーになる。`a` は中身と同じ分類になるので、ブロックを包んだ `a` はブロックが置ける場所にだけ置ける。ホストが渡す Html の型は `HtmlType` の Flow（`body` の中身）、Phrasing（文中に置けるもの）、Metadata（`head` の中身）のどれかで、ホストが Rust で組み立てる要素にも同じ規則を当てる。
- URL のスキームなど、値そのものに依存する検査は描画時に行う。
- `Type::validate` は、値がその型にちょうど合うかを確かめる。ホストは、検査に使った型と実際に渡す値がずれていないことを、これで確かめられる。

## HTML の安全性

- String は、シリアライザーが必ずエスケープする。Html は構築 API でしか作れず、テンプレートから HTML 文字列を Html にする手段（`raw`、`safe`）はない。
- 書ける属性は、全要素に共通の属性（`id`・`class`・`title`・`lang` など）、要素ごとに決めた属性（`a` の `href`・`target`・`rel` など）、`data-*`、`aria-*` だけ。`herf` のような綴りの誤りは、型の検査で見つかる。
- `href`・`src`・`cite` と、複数の URL を並べる `srcset`・`imagesrcset`・`ping` は、どの URL もスキームが `http`・`https`・`mailto` か相対 URL でなければエラーにする。
- `script`・`style` 要素、`on*` 属性、`style` 属性は、テンプレートからもホストの Rust からも作れない。`<style>` と JSON のデータ（JSON-LD など）だけは、ホストが `Html::style`・`Html::json` で作って渡す。`Html::json` が受け付けるのは、ブラウザーがスクリプトとして実行しない JSON の MIME タイプ（`application/json` と `application/…+json`）だけ。

## 機能を足すとき

機能の数は目標にしない。新しい構文、値の型、標準関数を足す前に、genbit の実際のテンプレートを置き換えるために必要かを確かめる。genbit が整形済みの値を渡すことで済むなら、言語には足さない。

## 開発

Rust の検証は `compose.yaml` の `cli` サービスで行う。

```sh
docker compose build cli
docker compose run --rm cli fmt --check
docker compose run --rm cli clippy --locked --all-targets --all-features -- -D warnings
docker compose run --rm cli test --locked --all-targets --all-features
```
