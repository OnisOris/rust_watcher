mod foo {
    pub fn run() {}
}

mod bar {
    pub fn run() {}
}

struct Engine;

impl Engine {
    fn run(&self) {}
}

fn main() {
    foo::run();
    bar::run();
    Engine.run();
}
