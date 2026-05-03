# 实验三报告：中间表示生成

## 1. 实验目标

本实验要求在 TeaLang 编译器已有的 AST 到 LLVM IR 生成流程上，补全新增语言特性的 IR 支持。根据实验要求，本次实现包含两部分：

1. 必做部分：支持 `f32` 浮点类型和 `as` 类型转换。
2. 三选一部分：选择实现 `for i in start..end` 范围循环。

最终目标是让编译器在对应 feature 开启时，能够正确完成 AST 解析、类型推断、IR 生成、LLVM 编译链接和运行结果比对。

## 2. 总体实现思路

本次改动沿着 TeaLang 的编译流水线逐层推进：

1. 在语法和 AST 层加入 `f32`、浮点字面量、`as` 转换表达式和 `for` 语句。
2. 在 IR 基础设施层扩展类型、操作数和语句，加入 `float`、浮点常量、浮点算术、浮点比较和类型转换指令。
3. 在类型推断阶段识别浮点表达式、转换表达式和 for 循环局部变量作用域。
4. 在函数体 IR 生成阶段，根据操作数类型选择整数或浮点 IR 指令，并在赋值、变量定义、返回和函数调用参数处插入必要的隐式转换。
5. 为 for 循环生成标准控制流块：测试块、循环体块、递增块和出口块。

设计上尽量保持已有主线行为不变。feature 关闭时，主线测试仍然按原语义运行；feature 开启时，新增测试进入测试集合。

## 3. 语法与 AST 扩展

### 3.1 语法文件

在 `src/tealang.pest` 中新增了以下语法元素：

- `kw_f32`：识别 `f32` 类型。
- `kw_as`：识别类型转换表达式中的 `as`。
- `kw_for` 和 `kw_in`：识别 for-in 循环。
- `op_range`：识别 `..` 范围运算符。
- `float_num`：识别浮点字面量，如 `3.14`、`2.0`、`.5`。
- `cast_expr`：表示 `expr as type`。
- `for_stmt`：表示 `for i in start..end { ... }`。

同时将 `type_spec` 扩展为可包含 `f32`，将 `code_block_stmt` 扩展为可包含 `for_stmt`，并将乘除表达式的基本项从 `expr_unit` 改为 `cast_expr`，使 `x as f32 / 2.0` 这类表达式能被正确解析。

### 3.2 AST 节点

在 AST 层新增：

- `BuiltIn::Float`：表示源语言中的 `f32`。
- `ExprUnitInner::Float(f32)`：表示浮点字面量。
- `CastExpr` 和 `ArithExprInner::CastExpr`：表示 `as` 类型转换。
- `ForStmt` 和 `CodeBlockStmtInner::For`：表示 for-in 循环。

同时补充了 AST 的 `Display` 和树形打印逻辑，使 `--emit ast` 能展示新增节点。

## 4. IR 基础设施扩展

### 4.1 类型系统

在 `src/ir/types.rs` 中新增 `Dtype::F32`，其 LLVM IR 文本形式为：

```llvm
float
```

同时将函数返回类型白名单从 `void | i32` 扩展为 `void | i32 | f32`，使返回 `f32` 的函数可以注册到 `Registry`。

### 4.2 操作数

在 `src/ir/value.rs` 中新增：

- `FloatConst`：表示浮点常量操作数。
- `Operand::FloatConst`：使浮点常量能作为 IR 指令操作数出现。

浮点常量输出时采用 LLVM 需要的十六进制浮点格式。对于 `f32`，实现中先将 `f64` 值截断为 `f32`，再扩展回 `f64` 取 bit pattern，避免 `3.14` 这类值因双精度常量无法精确表示为单精度而被 LLVM 拒绝。

### 4.3 IR 语句

在 `src/ir/stmt.rs` 中新增：

- `FloatBinOp`：对应 `fadd`、`fsub`、`fmul`、`fdiv`。
- `FCmpPredicate`：对应 `oeq`、`one`、`ogt`、`oge`、`olt`、`ole`。
- `StmtInner::FBiOp`：浮点二元运算。
- `StmtInner::FCmp`：浮点比较。
- `StmtInner::SIToFP`：`i32 -> float`。
- `StmtInner::FPToSI`：`float -> i32`。

并补齐了这些语句的构造函数、`Display` 输出、操作数遍历和操作数重写逻辑。

## 5. 类型转换与类型推断

### 5.1 AST 类型到 IR 类型

在 `src/ir/gen/conversions.rs` 中，将：

- `BuiltIn::Int` 映射到 `Dtype::I32`。
- `BuiltIn::Float` 映射到 `Dtype::F32`。

引用类型和复合类型仍沿用原有递归转换逻辑。

### 5.2 表达式类型推断

在 `src/ir/gen/type_infer.rs` 中加入：

- 浮点字面量类型为 `Dtype::F32`。
- `as` 表达式的结果类型为目标类型。
- 算术表达式中只要任一操作数为 `f32`，结果就是 `f32`，否则为 `i32`。
- `i32` 与 `f32` 在赋值、初始化等场景中视为可兼容，实际转换交给 IR 生成阶段插入指令。

此外，针对 `long_code2` 这类极长左结合算术表达式，将算术表达式的类型推断改成先沿左链迭代收集右操作数，再从左到右合并类型，避免递归过深导致栈溢出。

### 5.3 For 循环类型推断

for 循环的推断逻辑为：

1. 推断 `start` 和 `end` 表达式，要求二者兼容 `i32`。
2. 在循环体的 fork 环境中加入循环变量，类型固定为 `i32`。
3. 推断循环体语句。
4. 合并循环体环境回外层环境前移除循环变量，避免循环变量泄漏到循环外。

这与 while 循环的环境合并方式保持一致，但额外处理了循环变量的局部作用域。

## 6. 函数体 IR 生成

### 6.1 隐式转换辅助函数

在 `src/ir/gen/function_gen.rs` 中实现了三个辅助函数：

- `coerce_to_f32`：将 `i32/i1` 通过 `sitofp` 提升为 `float`。
- `coerce_to_i32`：将 `float` 通过 `fptosi` 转换为 `i32`。
- `coerce_to`：根据目标 `Dtype` 调用相应转换。

这些转换被用于：

- 局部变量定义。
- 赋值语句。
- 返回语句。
- 函数调用参数。
- 混合类型算术运算。
- 混合类型比较。

### 6.2 浮点字面量与函数调用

`ExprUnitInner::Float` 被降低为 `Operand::FloatConst`。函数调用表达式支持 `f32` 返回值，返回 `f32` 时分配 `Dtype::F32` 的临时寄存器。

函数调用参数会根据被调函数签名进行隐式转换。例如形参为 `f32`、实参为 `i32` 时会插入 `sitofp`。

### 6.3 浮点算术

对于二元算术表达式：

- 若两侧都是整数，继续生成 `add/sub/mul/sdiv`。
- 若任一侧是 `f32`，先将另一侧转换为 `f32`，再生成 `fadd/fsub/fmul/fdiv`。

同样，为避免超长表达式递归过深，IR 生成也改成迭代处理左结合算术链。

### 6.4 浮点比较

比较表达式中：

- 整数比较生成 `icmp`。
- 浮点或混合比较生成 `fcmp`，并使用 ordered 谓词，如 `ogt`、`oeq`。

比较结果仍为 `i1`，之后由原有布尔分支逻辑消费。

### 6.5 For 循环 IR 生成

`for i in start..end` 被降低为四个基本块：

1. `test_label`：读取循环变量和上界，生成 `icmp slt i32 i, end`。
2. `body_label`：执行循环体。
3. `incr_label`：执行 `i = i + 1`，然后跳回测试块。
4. `exit_label`：循环结束后的后继块。

实现中使用 `alloca/load/store` 管理循环变量和上界：

- `start` 和 `end` 在进入循环前各计算一次。
- 循环变量 `i` 存储在栈槽中，并注册到当前局部作用域。
- `continue` 的目标是 `incr_label`，确保先递增再继续判断。
- `break` 的目标是 `exit_label`。
- 循环结束后退出作用域，循环变量不泄漏。

## 7. 其他兼容性处理

新增 `Operand::FloatConst` 和浮点 IR 指令后，AArch64 后端中的若干 match 需要保持穷尽。本实验的新增浮点测试走 LLVM IR 路径，不要求 AArch64 后端生成浮点汇编，因此在 `src/asm/aarch64/function_generator.rs` 中对浮点操作数和浮点 IR 指令返回明确的 unsupported 错误，避免 Rust 编译器因非穷尽匹配报错，也避免静默生成错误汇编。

静态求值模块 `src/ir/gen/static_eval.rs` 也补充了 `CastExpr` 和 `Float` 的匹配分支，使全局初始化等静态路径保持编译完整。

## 8. 测试结果

本次在本地依次运行了以下测试，均通过：

```bash
cargo check --features float,for-loop
cargo test --features float -- --test-threads=1
cargo test --features for-loop -- --test-threads=1
cargo test -- --test-threads=1
```

测试结果：

- `cargo check --features float,for-loop`：通过。
- `cargo test --features float -- --test-threads=1`：35 个测试全部通过。
- `cargo test --features for-loop -- --test-threads=1`：35 个测试全部通过。
- `cargo test -- --test-threads=1`：30 个主线测试全部通过。

曾尝试执行 `cargo fmt`，但当前机器的 Rust toolchain 缺少 `rustfmt` 组件，提示 `cargo-fmt is not installed for the toolchain 'stable-aarch64-apple-darwin'`。随后执行 `git diff --check`，未发现空白格式错误。

## 9. 总结

本次实验完成了必做的 `f32` 与 `as` IR 生成，并选择实现了 for-in 循环。实现覆盖了从语法解析、AST 表示、类型转换、类型推断到 IR 生成的完整链路。浮点部分能正确生成 LLVM 的 `float`、`fadd/fsub/fmul/fdiv`、`fcmp`、`sitofp` 和 `fptosi` 指令；for 循环部分能正确处理循环变量作用域、`break`、`continue`、嵌套循环以及表达式边界。

最终所有必做浮点测试、所选 for-loop 测试和主线回归测试均通过。
