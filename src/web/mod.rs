mod autocomplete;
mod image_proxy;
mod index;
mod opensearch;
mod search;
mod settings;

use std::{convert::Infallible, net::SocketAddr, sync::Arc};

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::{get, post, MethodRouter},
    Router,
};
use axum_extra::extract::CookieJar;
use maud::{html, Markup, PreEscaped};
use tracing::info;

use crate::config::Config;

macro_rules! register_static_routes {
    ( $app:ident, $subdir:expr, $( $x:expr ),* ) => {
        {
            $(
                let $app = $app.route(
                    concat!("/", $x),
                    static_route(
                        include_str!(concat!("assets/", $x)),
                        guess_mime_type($x),
                        $subdir
                    ),
                );
            )*

            $app
        }
    };
}

pub async fn run(config: Config) {
    let bind_addr = config.bind;
    let subdirectory = config.subdirectory.clone();

    let config = Arc::new(config);

    fn static_route<S>(
        content: &'static str,
        content_type: &'static str,
        subdirectory: &str,
    ) -> MethodRouter<S, Infallible>
    where
        S: Clone + Send + Sync + 'static,
    {
        // FIXME: This is so bad
        let content = content.replace("<[SUBDIRECTORY>]", subdirectory);
        let response = ([(header::CONTENT_TYPE, content_type)], content);
        get(|| async { response })
    }

    let app = Router::new()
        .route("/", get(index::get))
        .route("/search", get(search::get))
        .route("/settings", get(settings::get))
        .route("/settings", post(settings::post))
        .route("/opensearch.xml", get(opensearch::route))
        .route("/autocomplete", get(autocomplete::route))
        .route("/image-proxy", get(image_proxy::route))
        .layer(middleware::from_fn_with_state(
            config.clone(),
            config_middleware,
        ))
        .with_state(config);

    let app = register_static_routes![
        app,
        &subdirectory,
        "style.css",
        "script.js",
        "robots.txt",
        "themes/catppuccin-mocha.css",
        "themes/catppuccin-latte.css",
        "themes/nord-bluish.css",
        "themes/discord.css"
    ];

    // Nest the app in a subdirectory chosen by the user
    let subdirectory_router = Router::new()
        .nest(&subdirectory, app);

    info!("Listening on http://{bind_addr}{subdirectory}");

    let listener = tokio::net::TcpListener::bind(bind_addr).await.unwrap();
    axum::serve(
        listener,
        subdirectory_router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

fn guess_mime_type(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "text/plain; charset=utf-8",
    }
}

async fn config_middleware(
    State(config): State<Arc<Config>>,
    cookies: CookieJar,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let mut config = config.clone().as_ref().clone();

    fn set_from_cookie(config: &mut String, cookies: &CookieJar, name: &str) {
        if let Some(cookie) = cookies.get(name) {
            let value = cookie.value();
            *config = value.to_string();
        }
    }

    set_from_cookie(&mut config.ui.stylesheet_url, &cookies, "stylesheet-url");
    set_from_cookie(&mut config.ui.stylesheet_str, &cookies, "stylesheet-str");

    // modify the state
    req.extensions_mut().insert(config);

    Ok(next.run(req).await)
}

pub fn head_html(title: Option<&str>, config: &Config) -> Markup {
    html! {
        head {
            meta charset="UTF-8";
            meta name="viewport" content="width=device-width, initial-scale=1.0";
            title {
                @if let Some(title) = title {
                    { (title) }
                    { " - " }
                }
                {(config.ui.site_name)}
            }
            link rel="stylesheet" href=(config.subdirectory.clone() + "/style.css");
            @if !config.ui.stylesheet_url.is_empty() {
                link rel="stylesheet" href=(config.ui.stylesheet_url);
            }
            @if !config.ui.stylesheet_str.is_empty() {
                style { (PreEscaped(html_escape::encode_style(&config.ui.stylesheet_str))) }
            }
            script src=(config.subdirectory.clone() + "/script.js") defer {}
            link rel="search" type="application/opensearchdescription+xml" title="metasearch" href=(config.subdirectory.clone() + "/opensearch.xml");
        }
    }
}
