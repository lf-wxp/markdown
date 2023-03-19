# format 格式化参数

位置、命名参数

```rust
format!("{1} {} {0} {}", 1, 2); // => "2 1 1 2"
format!("{a} {c} {b}", a="a", b='b', c=3);  // => "a 3 b"
```

宽度

字符填充默认左边对其，填充在右边
```rust
// All of these print "Hello x    !"
format!("Hello {:5}!", "x");
format!("Hello {:1$}!", "x", 5); // 其中的$符号表示width是前面的位置参数
format!("Hello {1:0$}!", 5, "x");
format!("Hello {:width$}!", "x", width = 5);
```

数字填充默认右边对其，填充在左边
```rust 
format!("Hello {:5}!", 5); // Hello     5!
format!("Hello {:05}!", 5); // Hello 00005!
format!("Hello {:+}!", 5); // Hello +5
format!("Hello {:05}!", -5); // 负号也要占用一位宽度 => Hello -0005!
```

填充与对其方式

```rust
format!("Hello {:<5}!", "x"); // "Hello x    !"
format!("Hello {:-<5}!", "x");// "Hello x----!"
format!("Hello {:^5}!", "x"); // "Hello   x  !"
format!("Hello {:>5}!", "x"); //  "Hello     x!
```

标志位 #

```rust
format!("Hello {:+}!", 5); // "Hello +5!"
format!("{:#x}!", 27); // "0x1b!"
format!("{:x}!", 27); // "1b!"
format!("{:X}!", 27); // "1B!"
format!("Hello {:05}!", 5); // "Hello 00005!"  这的0表示padding的字符，只有0是这样
format!("Hello {:15}!", 5); // "Hello              5!" 这里的15就是width
format!("Hello {:05}!", -5); // "Hello -0005!"
format!("{:#010x}!", 27); // "0x0000001b!" 0 是填充字符，10是width参数
```

捕获环境变量
有局限，它只能捕获普通的变量，对于更复杂的类型（例如表达式），可以先将它赋值给一个变量
```rust
fn get_person() -> String {
    String::from("sunface")
}
fn main() {
    let person = get_person();
    println!("Hello, {person}!");
}
```

- `#` - This flag indicates that the “alternate” form of printing should be used. The alternate forms are:
  
  - `#?` - pretty-print the [`Debug`](https://doc.rust-lang.org/std/fmt/trait.Debug.html "Debug") formatting (adds linebreaks and indentation)
  - `#x` - precedes the argument with a `0x`
  - `#X` - precedes the argument with a `0x`
  - `#b` - precedes the argument with a `0b`
  - `#o` - precedes the argument with a `0o`

- `0` - This is used to indicate for <mark>integer formats</mark> that the padding to `width` should both be done with a `0` character as well as be sign-aware. A format like `{:08}` would yield `00000001` for the integer `1`, while the same format would yield `-0000001` for the integer `-1`. Notice that the negative version has one fewer zero than the positive version. Note that padding zeros are always placed after the sign (if any) and before the digits. When used together with the `#` flag, a similar rule applies: padding zeros are inserted after the prefix but before the digits. The prefix is included in the total width.

精度

1. An integer `.N`:
   
   the integer `N` itself is the precision.

2. An integer or name followed by dollar sign `.N$`:
   
   use format *argument* `N` (which must be a `usize`) as the precision.

3. An asterisk `.*`:
   
   `.*` means that this `{...}` is associated with *two* format inputs rather than one: the first input holds the `usize` precision, and the second holds the value to print. Note that in this case, if one uses the format string `{<arg>:<spec>.*}`, then the `<arg>` part refers to the *value* to print, and the `precision` must come in the input preceding `<arg>`.

```rust
// all of these print Hello x is 0.01000
// Hello {arg 0 ("x")} is {arg 1 (0.01) with precision specified inline (5)}
format!("Hello {0} is {1:.5}", "x", 0.01);

// Hello {arg 1 ("x")} is {arg 2 (0.01) with precision specified in arg 0 (5)}
format!("Hello {1} is {2:.0$}", 5, "x", 0.01);

// Hello {arg 0 ("x")} is {arg 2 (0.01) with precision specified in arg 1 (5)}
format!("Hello {0} is {2:.1$}", "x", 5, 0.01);

// Hello {next arg ("x")} is {second of next two args (0.01) with precision
//                          specified in first of next two args (5)}
format!("Hello {} is {:.*}",    "x", 5, 0.01);

// Hello {next arg ("x")} is {arg 2 (0.01) with precision
//                          specified in its predecessor (5)}
format!("Hello {} is {2:.*}",   "x", 5, 0.01);

// Hello {next arg ("x")} is {arg "number" (0.01) with precision specified
//                          in arg "prec" (5)}
format!("Hello {} is {number:.prec$}", "x", prec = 5, number = 0.01);
```

如果在使用精度格式化的时候，被格式化的值不是数字，是字符串数字，则会按照字符串长度截取

```rust
// print Hello, `123` has 3 characters
format!("{}, `{name:.*}` has 3 characters", "Hello", 3, name="1234.56");
```

其他一些formatting trait

- *nothing* ⇒ [`Display`](https://doc.rust-lang.org/std/fmt/trait.Display.html "Display")
- `?` ⇒ [`Debug`](https://doc.rust-lang.org/std/fmt/trait.Debug.html "Debug")
- `x?` ⇒ [`Debug`](https://doc.rust-lang.org/std/fmt/trait.Debug.html "Debug") with lower-case hexadecimal integers
- `X?` ⇒ [`Debug`](https://doc.rust-lang.org/std/fmt/trait.Debug.html "Debug") with upper-case hexadecimal integers
- `o` ⇒ [`Octal`](https://doc.rust-lang.org/std/fmt/trait.Octal.html "Octal")
- `x` ⇒ [`LowerHex`](https://doc.rust-lang.org/std/fmt/trait.LowerHex.html "LowerHex")
- `X` ⇒ [`UpperHex`](https://doc.rust-lang.org/std/fmt/trait.UpperHex.html "UpperHex")
- `p` ⇒ [`Pointer`](https://doc.rust-lang.org/std/fmt/trait.Pointer.html "Pointer")
- `b` ⇒ [`Binary`](https://doc.rust-lang.org/std/fmt/trait.Binary.html "Binary")
- `e` ⇒ [`LowerExp`](https://doc.rust-lang.org/std/fmt/trait.LowerExp.html "LowerExp")
- `E` ⇒ [`UpperExp`](https://doc.rust-lang.org/std/fmt/trait.UpperExp.html "UpperExp")
