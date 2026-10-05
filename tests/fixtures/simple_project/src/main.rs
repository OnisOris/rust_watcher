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

fn chain_a() {
    chain_b();
}

fn chain_b() {
    chain_c();
}

fn chain_c() {}

mod first {
    pub fn run() {}
}

mod second {
    pub fn run() {}
}

fn invoke_first() {
    first::run();
}
