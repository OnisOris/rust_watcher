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

struct DemoRouter;

struct RouteHandler;

impl DemoRouter {
    fn new() -> Self {
        Self
    }

    fn route(self, _path: &str, _handler: RouteHandler) -> Self {
        self
    }
}

fn get<T>(_handler: T) -> RouteHandler {
    RouteHandler
}

fn post<T>(_handler: T) -> RouteHandler {
    RouteHandler
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

fn get_person() -> Person {
    Person::new("Ada", 29, 170.5)
}

fn update_person() -> Person {
    Person::new("Grace", 37, 168.0)
}

fn api_router() -> DemoRouter {
    DemoRouter::new()
        .route("/api/person", get(get_person))
        .route("/api/person/:name", post(update_person))
}

fn main() {
    let mut person = Person::new("Ada", 29, 170.5);
    person.birthday();
    print_greeting(&person);
    let _router = api_router();
}
