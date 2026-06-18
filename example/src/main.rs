#[derive(Debug, Clone)]
struct Person {
    name: String,
    age: u32,
    height_cm: f32,
}

#[derive(Debug)]
enum Mood {
    Happy,
    Focused,
}

trait Greeter {
    fn greeting(&self) -> String;
}

impl Person {
    fn new(name: &str, age: u32, height_cm: f32) -> Self {
        Self {
            name: name.to_string(),
            age,
            height_cm,
        }
    }

    fn birthday(&mut self) {
        self.age += 1;
    }
}

impl Greeter for Person {
    fn greeting(&self) -> String {
        format!(
            "Hello, I am {} and I am {} cm tall",
            self.name, self.height_cm
        )
    }
}

fn classify(person: &Person) -> Mood {
    if person.age > 30 {
        Mood::Focused
    } else {
        Mood::Happy
    }
}

fn print_greeting(person: &Person) {
    let mood = classify(person);
    println!("{} / {:?}", person.greeting(), mood);
}

fn main() {
    let mut person = Person::new("Ada", 29, 170.5);
    person.birthday();
    print_greeting(&person);
}
