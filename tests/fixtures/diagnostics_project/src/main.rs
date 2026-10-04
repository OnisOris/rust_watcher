fn broken() {
    let value: String = 42;
    println!("{value}");
}

fn warning() {
    let unused = 1;
}

fn main() {
    broken();
    warning();
}
