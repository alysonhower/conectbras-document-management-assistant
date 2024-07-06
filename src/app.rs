use leptos::*;

mod components;
use components::hero::*;

#[component]
pub fn App() -> impl IntoView {
    view! {
        <main class="container mx-auto">
            <Hero/>
        </main>
    }
}
