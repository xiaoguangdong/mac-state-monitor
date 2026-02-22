use sysinfo::Components;

fn main() {
    let components = Components::new_with_refreshed_list();
    for comp in &components {
        println!("label: {:?}, temp: {:?}", comp.label(), comp.temperature());
    }
    if components.iter().count() == 0 {
        println!("No temperature sensors found via sysinfo");
    }
}
