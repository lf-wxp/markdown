# 生命周期
```rust
#![allow(unused)]
fn main() {
  &i32        // 一个引用
  &'a i32     // 具有显式生命周期的引用
  &'a mut i32 // 具有显式生命周期的可变引用
}
```
生命周期语法用来将函数的多个引用参数和返回值的作用域关联到一起，一旦关联到一起后，Rust 就拥有充分的信息来确保我们的操作是内存安全的。

## 生命周期省略规则

三条消除规则
编译器使用三条消除规则来确定哪些场景不需要显式地去标注生命周期。其中第一条规则应用在输入生命周期上，第二、三条应用在输出生命周期上。若编译器发现三条规则都不适用时，就会报错，提示你需要手动标注生命周期。

- <strong>每一个引用参数都会获得独自的生命周期</strong>

  例如一个引用参数的函数就有一个生命周期标注: fn foo<'a>(x: &'a i32)，两个引用参数的有两个生命周期标注:fn foo<'a, 'b>(x: &'a i32, y: &'b i32), 依此类推。

- <strong>若只有一个输入生命周期(函数参数中只有一个引用类型)，那么该生命周期会被赋给所有的输出生命周期，也就是所有返回值的生命周期都等于该输入生命周期</strong>

  例如函数 fn foo(x: &i32) -> &i32，x 参数的生命周期会被自动赋给返回值 &i32，因此该函数等同于 fn foo<'a>(x: &'a i32) -> &'a i32

- <strong>若存在多个输入生命周期，且其中一个是 &self 或 &mut self，则 &self 的生命周期被赋给所有的输出生命周期</strong>

  拥有 &self 形式的参数，说明该函数是一个 方法，该规则让方法的使用便利度大幅提升。

## 生命周期约束语法

- 'a: 'b，是生命周期约束语法，跟泛型约束非常相似，用于说明 'a 必须比 'b 活得久
- 可以把 'a 和 'b 都在同一个地方声明（如上），或者分开声明但通过 where 'a: 'b 约束生命周期关系。

```
impl<'a: 'b, 'b> ImportantExcerpt<'a> {
    fn announce_and_return_part(&'a self, announcement: &'b str) -> &'b str {
        println!("Attention please: {}", announcement);
        self.part
    }
}

impl<'a> ImportantExcerpt<'a> {
    fn announce_and_return_part<'b>(&'a self, announcement: &'b str) -> &'b str
    where
        'a: 'b,
    {
        println!("Attention please: {}", announcement);
        self.part
    }
}
```
- T: 'a
  表示类型 T 必须比 'a 活得要久
  ```rust
  // Rust 2015
  struct Ref<'a, T: 'a> {
      field: &'a T
  }

  // Rust 2018
  struct Ref<'a, T> {
      field: &'a T
  }
  ```
# &‘static
`&'static` 对于生命周期有着非常强的要求：一个<mark>引用</mark>必须要活得跟剩下的程序一样久，才能被标注为 `&'static`。


`&'static` 生命周期针对的仅仅是<mark>引用</mark>，而不是持有该引用的变量，对于<mark>变量</mark>来说，还是要遵循相应的作用域规则.
变量销毁后，还是可以通过引用的地址获取到引用指向的数据的。

# T: 'static
`T: 'static` 与 `&'static` 有相同的约束：`T` 必须活得和程序一样久。

static 到底针对谁？
大家有没有想过，到底是 `&'static` 这个<mark>引用</mark>还是该<mark>引用指向的数据</mark>活得跟程序一样久呢？
答案是引用指向的数据，而引用本身是要遵循其作用域范围的.

# Reborrow 再借用

```rust
#[derive(Debug)]
struct Point {
    x: i32,
    y: i32,
}

impl Point {
    fn move_to(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
    }
}

fn main() {
    let mut p = Point { x: 0, y: 0 };
    let r = &mut p;
    // reborrow! 此时对`r`的再借用不会导致跟上面的借用冲突
    let rr: &Point = &*r;

    // 再借用`rr`最后一次使用发生在这里，在它的生命周期中，我们并没有使用原来的借用`r`，因此不会报错
    println!("{:?}", rr);

    // 再借用结束后，才去使用原来的借用`r`
    r.move_to(10, 10);
    println!("{:?}", r);
}
```

