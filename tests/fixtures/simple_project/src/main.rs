fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn calculate() -> i32 {
    add(1, 2)
}

fn привет() -> &'static str {
    "🦀"
}

fn main() {
    println!("{}", calculate());
    println!("{}", привет());
}
