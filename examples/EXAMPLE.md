# Markdown Preview Sample

This file contains neutral Markdown examples.

## Heading Levels

# Heading Level 1

## Heading Level 2

### Heading Level 3

#### Heading Level 4

##### Heading Level 5

###### Heading Level 6

## Paragraphs and Inline Styles

This paragraph contains **bold text**, _italic text_, **_bold italic text_**, ~~strikethrough text~~, and `inline code`.
It also includes HTML entities such as Fish &amp; Chips, tea &lt; coffee, and escaped characters like \*asterisks\* and \_underscores\_.

This line ends with a hard break.  
This line appears directly below it.

This line ends with a backslash\
and continues on the next line.

## Links and Images

[Inline link](https://example.com)

[Link with title](https://example.com/title "Example Title")

[Reference link][reference]

<https://example.com/help>

https://example.com/status?view=full

![Image example](https://example.com/image.png)

## Blockquotes

> Blockquote line 1
> Blockquote line 2
>
> > Nested blockquote line 1
> > Nested blockquote line 2
>
> - Blockquote list item 1
> - Blockquote list item 2

## Lists

### Unordered List

- Item 1
- Item 2
  - Nested item 2.1
  - Nested item 2.2
    - Nested item 2.2.1
- Item 3
  continuation line

### Ordered List

1. Item 1
2. Item 2
   1. Nested item 2.1
   2. Nested item 2.2
3. Item 3

1) Alternate marker 1
2) Alternate marker 2

### Task List

- [x] Completed task
- [ ] Incomplete task
  - [x] Nested completed task
  - [ ] Nested incomplete task

1. [x] Ordered completed task
2. [ ] Ordered incomplete task

- List item with blockquote child
  > Nested blockquote inside a list item
- List item with code fence child
  ```bash
  printf 'sample\n'
  ```

## Tables

| Column  | Center |       Right |
| :------ | :----: | ----------: |
| Value A |   1    |       alpha |
| Value B |   2    |        beta |
| 日本語  |   3    | mixed ASCII |

## Code Fences

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
Plain text fence
Line 2 with symbols like | and *.
Line 3 with indentation:
    plain text stays as-is.
```

## Mermaid Diagrams

The following Mermaid diagrams are rendered as ASCII art in place.

### Flowchart

#### Basic TD direction

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

#### Basic LR direction

```mermaid
flowchart LR
    Src(Markdown) --> Lex[Lexer]
    Lex --> AST[AST Builder]
    AST --> Render[Renderer]
    Render --> Out([ANSI output])
```

#### Nested subgraph

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

### Sequence diagram

#### Basic

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

#### With `alt` / `else` / `par` and a note

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

### Class diagram

#### Basic

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

#### With namespace, annotation, and static/abstract members

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

### State diagram

#### Basic

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

#### Composite state

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

### ER diagram

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

When the terminal supports color, each branch is rendered in its own color.

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

### xychart

```mermaid
xychart
title "Quarterly Performance"
x-axis ["Q1", "Q2", "Q3", "Q4"]
y-axis 0 --> 100
bar [30, 50, 40, 60]
line [35, 45, 55, 65]
```

The `horizontal` orientation swaps the axes:

```mermaid
xychart horizontal
title "Monthly Revenue"
x-axis "Month" [Jan, Feb, Mar]
y-axis "Revenue" 0 --> 300
bar [120, 200, 260]
```

## Horizontal Rule

---

## Closing Line

End of sample.

[reference]: https://example.com/reference "Reference Title"
