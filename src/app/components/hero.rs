use anyhow::{anyhow, Result};
use ev::MouseEvent;
use js_sys::Array;
use leptos::logging::log;
use leptos::*;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_wasm_bindgen::to_value;
use wasm_bindgen::prelude::*;
use web_sys::{Blob, Url};

#[derive(Serialize, Deserialize)]
struct GreetArgs<'a> {
    name: &'a str,
}

#[derive(Serialize, Deserialize)]
struct DocumentPath {
    path: String,
}

#[wasm_bindgen(js_namespace = ["window"])]
extern "C" {
    #[derive(Debug, Clone)]
    type TauriInstance;
    #[wasm_bindgen(js_name = "__TAURI__")]
    static TAURI_INSTANCE: TauriInstance;

    #[wasm_bindgen(getter, method)]
    fn core(this: &TauriInstance) -> TauriCoreApi;

    #[wasm_bindgen(getter, method)]
    fn event(this: &TauriInstance) -> TauriEventApi;
}

#[wasm_bindgen]
extern "C" {
    #[derive(Debug, Clone)]
    type TauriCoreApi;

    #[wasm_bindgen(catch, method)]
    async fn invoke(this: &TauriCoreApi, fn_name: &str, args: &JsValue)
        -> Result<JsValue, JsValue>;
}

#[wasm_bindgen]
extern "C" {
    #[derive(Debug, Clone)]
    type TauriEventApi;

    #[wasm_bindgen(catch, method)]
    async fn listen(
        this: &TauriEventApi,
        event_name: &str,
        callback: &Closure<dyn FnMut(JsValue)>,
    ) -> Result<JsValue, JsValue>;
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct EventData<T> {
    event: String,
    payload: T,
}

#[derive(Serialize, Deserialize)]
struct Log {
    message: String,
}

fn log_trace(message: &String) {
    let args = to_value(&Log {
        message: message.to_string(),
    })
    .unwrap();
    spawn_local(async move {
        if let Err(err) = invoke::<String>("log_trace", &args).await {
            log!("{}", err.to_string());
        }
    });
}

fn log_error(message: String) {
    let args = to_value(&Log { message }).unwrap();
    spawn_local(async move {
        if let Err(err) = invoke::<String>("log_error", &args).await {
            log!("{}", err.to_string());
        }
    });
}

async fn listen<F, T>(
    event_name: &str,
    mut callback: F,
) -> Result<Closure<dyn FnMut(wasm_bindgen::JsValue)>>
where
    F: FnMut(T) + 'static,
    T: DeserializeOwned,
{
    let callback = Closure::new(move |data: JsValue| {
        let data: Result<EventData<T>> =
            serde_wasm_bindgen::from_value(data).map_err(|err| anyhow!("{:?}", err));
        match data {
            Ok(data) => callback(data.payload),
            Err(err) => log_error(err.to_string()),
        }
    });

    TAURI_INSTANCE
        .event()
        .listen(event_name, &callback)
        .await
        .map_err(|err| anyhow!("{:?}", err))?;

    Ok(callback)
}

async fn invoke<T>(fn_name: &str, args: &JsValue) -> Result<T>
where
    T: DeserializeOwned,
{
    let result = TAURI_INSTANCE
        .core()
        .invoke(fn_name, args)
        .await
        .map_err(|err| anyhow!("{:?}", err))?;

    let output: Result<T> =
        serde_wasm_bindgen::from_value(result).map_err(|err| anyhow!("{:?}", err));

    match output {
        Ok(data) => Ok(data),
        Err(err) => {
            log_error(err.to_string());
            Err(err)
        }
    }
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
struct ImageLoaded {
    page_number: u16,
    path: String,
    data: Vec<u8>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
struct ImageUrl {
    page_number: u16,
    url: String,
}

#[component]
pub fn Hero() -> impl IntoView {
    let (page_number, set_page_number) = create_signal(1u16);
    let (images, set_images) = create_signal(Vec::<ImageUrl>::new());
    let selected_page = create_memo(move |_| {
        images.with(|urls| {
            urls.iter()
                .find(|url| page_number.with(|n| &url.page_number == n))
                .cloned()
        })
    });

    create_effect(move |_| {
        spawn_local(async move {
            let callback = listen("image", move |image: ImageLoaded| {
                let url = create_object_url(image.data);
                let page_number = image.page_number;
                set_images.update(|urls| urls.push(ImageUrl { page_number, url }));
            })
            .await
            .unwrap();
            callback.forget();
        });
    });

    let select_document =
        create_action(|input: &(WriteSignal<Vec<ImageUrl>>, WriteSignal<u16>)| {
            let set_images = input.0.clone();
            let set_page_number = input.1.clone();
            async move {
                let command = invoke::<String>("select_document", &JsValue::default()).await;
                match command {
                    Ok(path) => {
                        set_images.update(|images| {
                            images.clear();
                        });
                        set_page_number(1);
                        path
                    }
                    Err(_) => todo!(),
                }
            }
        });

    fn create_object_url(data: Vec<u8>) -> String {
        let array = Array::new();
        array.push(&js_sys::Uint8Array::from(&data[..]));

        let blob = Blob::new_with_u8_array_sequence(&array).unwrap();
        Url::create_object_url_with_blob(&blob).unwrap()
    }

    let _next_page = move |_: MouseEvent| {
        if page_number() < (images.with(|images| images.len()) - 1) as u16 {
            set_page_number.update(|page_number| *page_number += 1);
            let message = format!("Page_number: {}", page_number());
            log_trace(&message);
        }
    };

    let _previous_page = move |_: MouseEvent| {
        if page_number() > 1 {
            set_page_number.update(|page_number| *page_number -= 1);
            let message = format!("Page_number: {}", page_number());
            log_trace(&message);
        }
    };

    let path = select_document.value();
    let _preparing_document = select_document.pending();

    let _prepare_document = create_resource(path, |path| async move {
        match path {
            Some(path) => {
                let args = to_value(&DocumentPath { path }).ok()?;
                invoke::<String>("prepare_document", &args).await.ok()
            }
            None => None,
        }
    });

    view! {
        <div class="hero bg-base-200 min-h-screen">
            <div class="hero-content text-center">
                <div class="max-w-md">
                    {move || match selected_page().is_some() {
                        false => {
                            view! {
                                <h1 class="text-4xl font-bold">"Inicio"</h1>
                                <p class="py-6">"Para começar, selecione um documento."</p>
                                <button
                                    class="btn btn-primary"
                                    on:click=move |ev| {
                                        ev.prevent_default();
                                        select_document.dispatch((set_images, set_page_number));
                                    }
                                >

                                    "Selecionar documento"
                                </button>
                            }
                                .into_view()
                        }
                        true => {
                            view! {
                                <img
                                    src=move || selected_page().unwrap().url
                                    alt="Loaded image"
                                    style="width: 1000px; height: auto;"
                                />
                            }
                                .into_view()
                        }
                    }}
                    <button
                        class=("hidden", move || selected_page().is_none())
                        class="absolute bottom-24 right-4 btn btn-primary"
                        on:click=move |ev| {
                            ev.prevent_default();
                            select_document.dispatch((set_images, set_page_number));
                        }
                    >

                        "Selecionar documento"
                    </button>
                    <button

                        class=("hidden", move || selected_page().is_none())
                        class="absolute bottom-4 left-4 btn btn-primary"
                        on:click=_previous_page
                    >
                        "Página anterior"
                    </button>
                    <button

                        class=("hidden", move || selected_page().is_none())
                        class="absolute bottom-4 left-4 btn btn-primary"
                        class:hidden=move || selected_page().is_none()
                        class="absolute bottom-4 right-4 btn btn-primary"
                        on:click=_next_page
                    >
                        "Próxima página"
                    </button>
                </div>
            </div>
        </div>
    }
}
