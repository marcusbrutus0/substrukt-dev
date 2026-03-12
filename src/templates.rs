use minijinja::{Environment, path_loader};

pub fn create_environment() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_loader(path_loader("templates/"));
    env
}
