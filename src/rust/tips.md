
# ?

? 可以用于Result 和 Option的传播

# mod

在 Rust 中，父模块完全无法访问子模块中的私有项，但是子模块却可以访问父模块、父父..模块的私有项。

# 'static
使用 Box::leak 也可以产生 'static 生命周期

# 切片和切片引用
Rust 语言特性内置的 str 和 [u8] 类型都是切片，前者是字符串切片，后者是数组切片
在 Rust 中，所有的切片都是动态大小类型，它们都无法直接被使用

# 堆与栈
Rust 堆上对象还有一个特殊之处，它们都拥有一个所有者，因此受所有权规则的限制：当赋值时，发生的是所有权的转移（只需浅拷贝栈上的引用或智能指针即可）,底层数据并不会被拷贝，转移所有权仅仅是复制一份栈中的指针，再将新的指针赋予新的变量，然后让拥有旧指针的变量失效，最终完成了所有权的转移

当栈上数据转移所有权时，实际上是把数据拷贝了一份，最终新旧变量各自拥有不同的数据，因此所有权并未转移。


在 Rust 中，想实现不同类型组成的数组只有两个办法：枚举和特征对象，前者限制较多，因此后者往往是最常用的解决办法。