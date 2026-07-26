// 编译 ui/app.slint 为 Rust 代码
fn main() {
    slint_build::compile("ui/app.slint").expect("Slint UI compilation failed");
}
