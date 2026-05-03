# 实验二 类型推断实验报告

## 1. 实验目标

本实验的目标是在 TeaLang 编译器中实现函数返回类型推断，使省略返回类型标注的函数能够根据函数体中的 `return` 语句自动推断返回类型。

该功能通过 Cargo feature `return-type-inference` 开启，默认关闭。feature 关闭时，TeaLang 仍保持原有语义：省略返回类型表示返回 `void`。feature 开启后，编译器会在函数签名注册之后、函数体 IR 生成之前插入一个返回类型推断 Pass。

本次实验的交付要求是仅修改：

```text
src/experimental/return_infer.rs
```

我的 `lab2` 分支中，除本报告外，实验实现代码也只修改了该文件。

## 2. 实验思路

已有的局部变量类型推断是前向流推断：逐条语句处理局部变量声明、定义和赋值，并要求函数调用的返回类型已经在 `Registry` 中是具体的 `Dtype`。

函数返回类型推断的难点在于：

- 函数签名注册阶段还没有分析函数体，因此无法直接知道省略返回类型的函数到底返回 `i32` 还是 `void`。
- 函数之间可能互相调用，返回类型可能跨函数传播。
- 函数可能自递归，例如 `pow` 在函数体内调用自身，此时返回类型依赖自身的约束。

为了解决这些问题，我采用了“类型变量 + 全局约束求解”的方法：

1. 为每个省略返回类型的函数分配一个类型变量 `alpha_f`。
2. 遍历所有函数体，收集类型相等约束。
3. 使用并查集维护类型变量的等价类。
4. 在遍历结束后统一求解，把每个函数的返回类型写回 `Registry`。

这样，Pass 3 中原有的 `type_infer::infer_function` 和 `FunctionGenerator` 仍然只会看到具体的 `Dtype`，不需要修改原有 IR 生成逻辑。

## 3. 核心数据结构

### 3.1 Ty

在 `return_infer.rs` 内部使用 `Ty` 表示推断过程中的类型：

```rust
enum Ty {
    Concrete(Dtype),
    Var(TypeId),
}
```

其中：

- `Ty::Concrete(Dtype)` 表示已经确定的具体类型。
- `Ty::Var(TypeId)` 表示尚未确定的类型变量。

`Ty` 只存在于返回类型推断 Pass 内部，不泄漏到 `Registry`、`type_infer.rs` 或 `FunctionGenerator`。

### 3.2 UnionFind

并查集维护类型变量的等价类：

```rust
struct UnionFind {
    parent: Vec<TypeId>,
    rank: Vec<u32>,
    concrete: Vec<Option<Dtype>>,
}
```

每个等价类的根节点可以携带一个具体类型 `Option<Dtype>`。核心不变式是：每个等价类最多只能绑定一个具体类型。

我实现了以下操作：

- `fresh`：创建新的类型变量。
- `find`：查找等价类根，并做路径压缩。
- `bind`：把具体类型绑定到某个等价类。
- `union`：合并两个等价类，并检查具体类型是否冲突。
- `resolve`：查询等价类最终绑定的具体类型。

如果一个等价类已经绑定为 `void`，之后又试图绑定为 `i32`，会返回 `Error::TypeMismatch`。这正是负测试 `type_infer_5` 需要捕获的情况。

## 4. 约束求解

所有类型相等关系都通过 `unify` 进入并查集：

```rust
fn unify(uf: &mut UnionFind, a: &Ty, b: &Ty, symbol: &str) -> Result<(), Error>
```

`unify` 分三种情况处理：

- `Concrete` 和 `Concrete`：直接比较是否相等。
- `Var` 和 `Concrete`：调用 `bind`。
- `Var` 和 `Var`：调用 `union`。

这样，返回语句、变量定义、赋值、分支合并、循环合并、函数调用等产生的约束都使用同一套规则处理。

## 5. Pass 2.5 的实现

入口函数是：

```rust
resolve_return_types(registry, elements, globals)
```

它分为三步。

### 5.1 Seed

遍历所有 `FnDef`，对省略返回类型的函数分配类型变量：

```text
pending_returns[name] = alpha_f
```

如果没有函数省略返回类型，则直接返回，保持 feature 开启时对普通程序的零额外影响。

### 5.2 Collect

遍历所有函数体，而不只是 pending 函数体。原因是显式返回类型的函数也可能调用 pending 函数，这些调用点同样会产生约束。

每个函数体由 `Collector` 处理。`Collector` 保存：

- `registry`：函数签名与结构体类型。
- `globals`：全局变量。
- `pending`：省略返回类型函数到类型变量的映射。
- `uf`：全局共享的并查集。
- `return_var`：当前函数的返回类型变量，若当前函数显式标注返回类型则为 `None`。
- `env`：局部变量到 `Ty` 的映射。

主要收集规则如下：

- `return e;`：若当前函数 pending，则 `unify(alpha_self, typeOf(e))`。
- `return;`：若当前函数 pending，则 `unify(alpha_self, void)`。
- 调用 pending 函数：返回 `Ty::Var(alpha_callee)`。
- 调用非 pending 函数：从 `Registry` 取具体返回类型。
- `let x: T = e;`：`unify(T, typeOf(e))`。
- `let x = e;`：把 `typeOf(e)` 放入局部环境，允许它是类型变量。
- `x = e;`：`unify(typeOf(x), typeOf(e))`。
- `if/else` 合并：对分支前已有变量统一两边类型。
- `while` 合并：循环体类型必须与循环前类型兼容。

此外，我还补充了上下文约束：

- 算术表达式的左右操作数必须是 `i32`。
- 比较表达式的左右操作数必须是 `i32`。
- 数组初始化元素必须是 `i32`。
- 函数实参与参数类型做 `unify`。
- 数组元素或结构体成员赋值时，左值类型与右值类型做 `unify`。

这些约束可以让 pending 函数返回类型从使用上下文中被反向确定。

### 5.3 Resolve

遍历 `pending_returns`：

```rust
let dtype = uf.resolve(alpha_f).unwrap_or(Dtype::Void);
```

如果没有任何约束确定该函数返回类型，则回退为 `void`，保持旧语义兼容。

如果推断结果是 `void` 或 `i32`，就写回：

```text
registry.function_types[name].return_dtype
```

如果推断出其它类型，则报 `UnsupportedReturnType`。

## 6. 测试用例覆盖

本实现覆盖了实验要求中的全部测试点：

- `type_infer_basic`：所有函数都有显式返回类型，验证不破坏已有局部变量推断。
- `type_infer_1`：线性跨函数调用链，验证返回类型跨函数传播。
- `type_infer_2`：自递归，验证类型变量可以处理循环依赖。
- `type_infer_3`：`&[i32]` 引用参数，验证引用数组参数与返回类型推断共存。
- `type_infer_4`：三级调用链，验证推断结果可用于后续条件和循环。
- `type_infer_5`：同时存在 `return;` 和 `return t;`，验证 `void` 与 `i32` 冲突会被拒绝。

## 7. 测试结果

端到端测试会写入 `tests/*/build` 目录，因此不能并行运行两组 cargo test，否则测试产物可能互相覆盖。最终使用单测试线程顺序验证。

默认配置：

```bash
cargo test -- --test-threads=1
```

结果：

```text
30 passed; 0 failed
```

开启返回类型推断：

```bash
cargo test --features return-type-inference -- --test-threads=1
```

结果：

```text
35 passed; 0 failed
```

单独运行实验相关测试：

```bash
cargo test --features return-type-inference type_infer -- --test-threads=1
```

结果：

```text
6 passed; 0 failed
```

## 8. 总结

本实验在不修改主线类型推断与 IR 生成逻辑的前提下，通过一个 feature-gated 的 Pass 2.5 实现了函数返回类型推断。

实现的关键是把返回类型未知的问题转化为类型变量之间、类型变量与具体类型之间的等价约束，并用并查集统一求解。求解完成后再把结果写回 `Registry`，从而让后续 Pass 仍然面对普通的具体 `Dtype`。

最终结果满足实验要求：

- 只改动返回类型推断实现文件。
- 支持跨函数调用链。
- 支持自递归。
- 支持引用数组参数。
- 能检测冲突返回类型。
- feature 关闭和开启两种测试配置均通过。
