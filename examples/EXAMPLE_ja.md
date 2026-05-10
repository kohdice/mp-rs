# Markdownプレビューサンプル

このファイルは中立的な Markdown の例をまとめたものです。

## 見出しレベル

# 見出しレベル 1

## 見出しレベル 2

### 見出しレベル 3

#### 見出しレベル 4

##### 見出しレベル 5

###### 見出しレベル 6

## 段落とインライン装飾

この段落には**太字**、_斜体_、**_太字と斜体の組み合わせ_**、~~取り消し線~~、`インラインコード`が含まれています。
さらに Fish &amp; Chips、tea &lt; coffee のような HTML エンティティと、\*アスタリスク\* や \_アンダースコア\_ のようなエスケープ例も入れています。

この行はハードブレークで終わります。  
この行はその直下に表示されます。

この行はバックスラッシュで終わり\
次の行へ続きます。

## リンクと画像

[インラインリンク](https://example.com)

[タイトル付きリンク](https://example.com/title "Example Title")

[参照リンク][reference]

<https://example.com/help>

https://example.com/status?view=full

![画像の例](https://example.com/image.png)

## 引用

> 引用行 1
> 引用行 2
>
> > ネストした引用 1
> > ネストした引用 2
>
> - 引用内リスト 1
> - 引用内リスト 2

## リスト

### 順不同リスト

- 項目 1
- 項目 2
  - ネスト項目 2.1
  - ネスト項目 2.2
    - ネスト項目 2.2.1
- 項目 3
  継続行

### 順序付きリスト

1. 項目 1
2. 項目 2
   1. ネスト項目 2.1
   2. ネスト項目 2.2
3. 項目 3

1) 別マーカー 1
2) 別マーカー 2

### タスクリスト

- [x] 完了したタスク
- [ ] 未完了のタスク
  - [x] ネストした完了タスク
  - [ ] ネストした未完了タスク

1. [x] 順序付き完了タスク
2. [ ] 順序付き未完了タスク

- 引用子要素を持つ項目
  > リスト項目内のネストした引用
- コードフェンス子要素を持つ項目
  ```bash
  printf 'sample\n'
  ```

## 表

| 列     | 中央 |          右 |
| :----- | :--: | ----------: |
| 値 A   |  1   |       alpha |
| 値 B   |  2   |        beta |
| 日本語 |  3   | mixed ASCII |

## コードフェンス

```zig
const std = @import("std");

fn sum(values: []const i32) i32 {
    var total: i32 = 0;
    for (values) |value| total += value;
    return total;
}

pub fn main() void {
    const values = [_]i32{ 1, 2, 3, 4 };
    std.debug.print("sum={}\n", .{sum(&values)});
}
```

```c
#include <stdio.h>

static int sum(const int *values, int len) {
    int total = 0;
    for (int i = 0; i < len; ++i) {
        total += values[i];
    }
    return total;
}

int main(void) {
    int values[] = {1, 2, 3, 4};
    printf("sum=%d\n", sum(values, 4));
    return 0;
}
```

```rust
fn sum(values: &[i32]) -> i32 {
    values.iter().copied().sum()
}

fn main() {
    let values = [1, 2, 3, 4];
    println!("sum={}", sum(&values));
}
```

```go
package main

import "fmt"

func sum(values []int) int {
	total := 0
	for _, value := range values {
		total += value
	}
	return total
}

func main() {
	values := []int{1, 2, 3, 4}
	fmt.Printf("sum=%d\n", sum(values))
}
```

```json
{
  "name": "example",
  "enabled": true,
  "items": [
    { "id": 1, "label": "alpha" },
    { "id": 2, "label": "beta" }
  ],
  "meta": {
    "count": 2,
    "tag": "sample"
  }
}
```

```bash
set -eu

input="sample.md"

if [ -f "$input" ]; then
  mp "$input"
else
  printf 'missing: %s\n' "$input"
fi
```

```
プレーンテキストのフェンス
2 行目には | や * のような記号があります。
3 行目はインデント付きです。
    plain text stays as-is.
```

## Mermaid 図

以下の Mermaid 図は ASCII アートとしてその場で描画されます。

### フローチャート (flowchart)

#### 基本 (TD 方向)

```mermaid
flowchart TD
    A(Input) --> B[Lexer]
    B --> C[Parser]
    C --> D{Valid?}
    D -->|yes| E[Render]
    D -->|no| F[Report error]
    E --> G([Done])
    F --> G
```

#### 基本 (LR 方向)

```mermaid
flowchart LR
    Src(Markdown) --> Lex[Lexer]
    Lex --> AST[AST Builder]
    AST --> Render[Renderer]
    Render --> Out([ANSI output])
```

#### ネストした subgraph

```mermaid
flowchart TD
    subgraph services [ServicesLayer]
        Svc1[Receive request] --> Svc2[Validate payload]
        subgraph adapters [AdaptersLayer]
            A1[DB adapter] --> A2[Cache adapter]
        end
        Svc2 --> A1
    end
    A2 --> Out([Done])
```

### シーケンス図 (sequenceDiagram)

#### 基本

```mermaid
sequenceDiagram
    participant U as User
    participant B as Browser
    participant API
    participant DB

    U->>B: Open /login
    B->>API: POST /login
    API->>DB: SELECT user
    DB-->>API: user row
    API-->>B: 200 OK + token
    B-->>U: render dashboard
```

#### `alt` / `else` / `par` と note

```mermaid
sequenceDiagram
    participant Client
    participant API
    participant DB
    Client->>API: GET /resource
    alt cached
        API-->>Client: 200 OK (cached)
    else miss
        API->>DB: SELECT resource
        DB-->>API: row
        API-->>Client: 200 OK
    end
    par warm cache
        API->>DB: touch resource
    and record metrics
        API->>DB: insert metric
    end
    Note over Client,API: request completed
```

### クラス図 (classDiagram)

#### 基本

```mermaid
classDiagram
    class Repository {
        <<interface>>
        +findById(id) Entity
        +save(entity) void
        +delete(id) void
    }
    class UserRepository {
        -db Database
        +findById(id) User
        +save(user) void
        +delete(id) void
        +findByEmail(email) User
    }
    class User {
        +id int
        +email str
        +name str
        +hashedPassword str
        +verify(password) bool
    }
    Repository <|.. UserRepository
    UserRepository o-- User
```

#### namespace・annotation・static/abstract メンバー

```mermaid
classDiagram
    namespace Billing {
        class Account {
            <<abstract>>
            +String ownerId
            +int balance$
            +apply(Transaction) void
            +settle()*
        }
        class Transaction {
            +String id
            +int amount
            +describe() String
        }
    }
    Account o-- Transaction : records
```

### 状態遷移図 (stateDiagram)

#### 基本

```mermaid
stateDiagram-v2
    state "Waiting for payment" as Pending
    state "Payment confirmed" as Confirmed
    state "Being shipped" as Shipped

    [*] --> Pending
    Pending --> Confirmed : payment_received
    Confirmed --> Shipped : dispatched
    Shipped --> Delivered : arrived
    Delivered --> [*]
```

#### 複合状態 (composite state)

```mermaid
stateDiagram-v2
    [*] --> Idle
    state Active {
        [*] --> Waiting
        Waiting --> Working : request
        Working --> Waiting : finished
    }
    Idle --> Active : start
    Active --> Idle : stop
    Idle --> [*]
```

### ER 図 (erDiagram)

```mermaid
erDiagram
    authors ||--o{ books : writes
    categories ||--o{ book_categories : tags
    books ||--o{ book_categories : classified_as

    authors {
        INT id PK
        VARCHAR name
        VARCHAR email
        DATETIME created_at
    }
    books {
        INT id PK
        INT author_id FK
        VARCHAR title
        INT price
        DATE published_at
    }
    categories {
        INT id PK
        VARCHAR name
        VARCHAR slug
    }
    book_categories {
        INT book_id FK
        INT category_id FK
    }
```

### gitGraph

ターミナルがカラー対応していれば、branch ごとに色分けされて表示されます。

```mermaid
gitGraph
    commit id: "init"
    commit tag: "v0.9"
    branch develop
    commit
    branch feature
    commit
    commit
    checkout develop
    merge feature
    commit
    checkout main
    merge develop tag: "v1.0" type: HIGHLIGHT
    commit
```

### XY チャート (xychart)

```mermaid
xychart
title "Quarterly Performance"
x-axis ["Q1", "Q2", "Q3", "Q4"]
y-axis 0 --> 100
bar [30, 50, 40, 60]
line [35, 45, 55, 65]
```

`horizontal` を付けると軸を入れ替えた横向きレンダリングになります。

```mermaid
xychart horizontal
title "Monthly Revenue"
x-axis "Month" [Jan, Feb, Mar]
y-axis "Revenue" 0 --> 300
bar [120, 200, 260]
```

## 水平線

---

## 終了行

サンプルはここで終わりです。

[reference]: https://example.com/reference "Reference Title"
