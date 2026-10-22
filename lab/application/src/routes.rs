use std::collections::HashMap;

use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::{Form, Router};
use serde::Deserialize;
use serde_json::json;

use crate::auth::{self, AuthContext, AuthOutcome};
use crate::pages;
use crate::state::{self, App, SESSION_COOKIE, Tenant};

pub fn main_router(app: App) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/about", get(about))
        .route("/pricing", get(pricing))
        .route("/robots.txt", get(robots))
        .route("/sitemap.xml", get(sitemap))
        .route("/login", get(login_form).post(login))
        .route("/portal/signin", get(portal_form).post(portal_login))
        .route("/logout", get(logout))
        .route("/app", get(app_root))
        .route("/app/{tenant}", get(dashboard))
        .route("/app/{tenant}/reports", get(reports))
        .route("/app/{tenant}/api/reports", get(api_reports))
        .route("/app/{tenant}/settings", get(settings))
        .route("/app/{tenant}/items", get(items))
        .route("/app/{tenant}/admin/forbidden", get(admin_forbidden))
        .route("/app/{tenant}/prompt-injection", get(prompt_injection))
        .route("/app/{tenant}/secret-surface", get(secret_surface))
        .route("/app/{tenant}/expire", get(expire))
        .route("/app/{tenant}/counter", post(counter))
        .route("/app/{tenant}/api/state", get(api_state))
        .route("/app/{tenant}/effect-check", get(effect_check))
        .route("/app/{tenant}/burst", get(burst_page))
        .route("/app/{tenant}/api/burst", get(burst_api))
        .route("/app/{tenant}/api/slow-effect", post(slow_effect))
        .route("/go/outside", get(go_outside))
        .route("/assets/app.js", get(app_js))
        .route("/assets/reports.js", get(reports_js))
        .route("/assets/burst.js", get(burst_js))
        .route("/secret-note.txt", get(secret_note))
        .route("/excluded", get(main_excluded))
        .route("/_lab/state", get(lab_state))
        .route("/_lab/reset", post(lab_reset))
        .fallback(fallback)
        .layer(middleware::from_fn_with_state(app.clone(), track_hits))
        .layer(middleware::from_fn(security_headers))
        .with_state(app)
}

pub fn excluded_router(app: App) -> Router {
    Router::new()
        .route("/excluded", get(excluded))
        .route("/_lab/state", get(lab_state))
        .fallback(fallback)
        .layer(middleware::from_fn(security_headers))
        .with_state(app)
}

async fn track_hits(State(app): State<App>, request: Request, next: Next) -> Response {
    app.record_hit(request.uri().path());
    next.run(request).await
}

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(
            "default-src 'none'; style-src 'unsafe-inline'; script-src 'self'; connect-src 'self'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'",
        ),
    );
    response
}

fn html(status: StatusCode, body: String) -> Response {
    response_with_type(status, "text/html; charset=utf-8", body)
}

fn response_with_type(status: StatusCode, content_type: &'static str, body: String) -> Response {
    let mut response = body.into_response();
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
}

fn redirect(location: &str) -> Response {
    let mut response = StatusCode::FOUND.into_response();
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::try_from(location).expect("static redirect target"),
    );
    response
}

fn redirect_with_cookie(location: &str, cookie: String) -> Response {
    let mut response = redirect(location);
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::try_from(cookie).expect("cookie value is header-safe"),
    );
    response
}

// Response is the natural error type at the handler boundary.
#[allow(clippy::result_large_err)]
fn resolve_tenant(raw: &str) -> Result<Tenant, Response> {
    Tenant::parse(raw).ok_or_else(|| html(StatusCode::NOT_FOUND, pages::not_found()))
}

#[allow(clippy::result_large_err)]
fn require_auth(app: &App, headers: &HeaderMap, tenant: Tenant) -> Result<AuthContext, Response> {
    let outcome = {
        let mut state = app.lock();
        auth::authorize(&mut state, headers, tenant.as_str())
    };
    match outcome {
        AuthOutcome::Ok(context) => Ok(context),
        AuthOutcome::MissingSession | AuthOutcome::UnknownUser => Err(redirect("/login")),
        AuthOutcome::ExpiredSession => Err(redirect_with_cookie(
            "/login?expired=1",
            auth::clear_cookie(),
        )),
        AuthOutcome::WrongTenant => Err(html(
            StatusCode::FORBIDDEN,
            pages::message(
                "Forbidden",
                "Forbidden",
                "This account is not a member of that tenant.",
            ),
        )),
    }
}

async fn index() -> Response {
    html(StatusCode::OK, pages::index())
}

async fn about() -> Response {
    html(StatusCode::OK, pages::about())
}

async fn pricing() -> Response {
    html(StatusCode::OK, pages::pricing())
}

async fn robots() -> Response {
    response_with_type(
        StatusCode::OK,
        "text/plain; charset=utf-8",
        "User-agent: *\nDisallow: /_lab\nDisallow: /admin\nAllow: /\n".to_string(),
    )
}

async fn sitemap(State(app): State<App>) -> Response {
    let base = format!("http://127.0.0.1:{}", app.lab_port);
    let body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
<url><loc>{base}/</loc></url>
<url><loc>{base}/about</loc></url>
<url><loc>{base}/pricing</loc></url>
<url><loc>{base}/login</loc></url>
</urlset>"#
    );
    response_with_type(StatusCode::OK, "application/xml; charset=utf-8", body)
}

async fn login_form() -> Response {
    html(StatusCode::OK, pages::login())
}

async fn portal_form() -> Response {
    html(StatusCode::OK, pages::portal_login())
}

#[derive(Deserialize)]
struct PortalForm {
    account: String,
    passphrase: String,
}

async fn portal_login(State(app): State<App>, Form(form): Form<PortalForm>) -> Response {
    let account = form.account.trim();
    let (session_id, tenant) = {
        let mut state = app.lock();
        let Some(user) = state.user(account).cloned() else {
            tracing::warn!(user = ?account, "portal login rejected");
            return html(StatusCode::UNAUTHORIZED, pages::login_failed());
        };
        if user.password != form.passphrase {
            tracing::warn!(user = ?account, "portal login rejected");
            return html(StatusCode::UNAUTHORIZED, pages::login_failed());
        }
        let tenant = user.tenants.first().copied().unwrap_or("north").to_string();
        let session_id = state.create_session(&user);
        (session_id, tenant)
    };
    redirect_with_cookie(&format!("/app/{tenant}"), auth::session_cookie(&session_id))
}

#[derive(Deserialize)]
struct LoginForm {
    username: String,
    password: String,
}

async fn login(State(app): State<App>, Form(form): Form<LoginForm>) -> Response {
    let username = form.username.trim();
    let (session_id, tenant) = {
        let mut state = app.lock();
        let Some(user) = state.user(username).cloned() else {
            tracing::warn!(user = ?username, "lab login rejected");
            return html(StatusCode::UNAUTHORIZED, pages::login_failed());
        };
        if user.password != form.password {
            tracing::warn!(user = ?username, "lab login rejected");
            return html(StatusCode::UNAUTHORIZED, pages::login_failed());
        }
        let tenant = user.tenants.first().copied().unwrap_or("north").to_string();
        let session_id = state.create_session(&user);
        (session_id, tenant)
    };
    tracing::info!(user = %username, tenant = %tenant, "lab login accepted");
    redirect_with_cookie(&format!("/app/{tenant}"), auth::session_cookie(&session_id))
}

async fn logout(State(app): State<App>, headers: HeaderMap) -> Response {
    if let Some(session_id) = auth::cookie_value(&headers, SESSION_COOKIE) {
        app.lock().sessions.remove(&session_id);
    }
    redirect_with_cookie("/", auth::clear_cookie())
}

async fn app_root() -> Response {
    redirect("/login")
}

async fn dashboard(
    State(app): State<App>,
    headers: HeaderMap,
    Path(tenant): Path<String>,
) -> Response {
    let tenant = match resolve_tenant(&tenant) {
        Ok(tenant) => tenant,
        Err(response) => return response,
    };
    let context = match require_auth(&app, &headers, tenant) {
        Ok(context) => context,
        Err(response) => return response,
    };
    html(StatusCode::OK, pages::dashboard(&context.user, tenant))
}

async fn reports(
    State(app): State<App>,
    headers: HeaderMap,
    Path(tenant): Path<String>,
) -> Response {
    let tenant = match resolve_tenant(&tenant) {
        Ok(tenant) => tenant,
        Err(response) => return response,
    };
    let context = match require_auth(&app, &headers, tenant) {
        Ok(context) => context,
        Err(response) => return response,
    };
    if !context.user.role.is_admin() {
        return html(StatusCode::FORBIDDEN, pages::role_required());
    }
    html(StatusCode::OK, pages::reports(tenant))
}

async fn settings(
    State(app): State<App>,
    headers: HeaderMap,
    Path(tenant): Path<String>,
) -> Response {
    let tenant = match resolve_tenant(&tenant) {
        Ok(tenant) => tenant,
        Err(response) => return response,
    };
    if let Err(response) = require_auth(&app, &headers, tenant) {
        return response;
    }
    html(StatusCode::OK, pages::settings(tenant))
}

async fn items(
    State(app): State<App>,
    headers: HeaderMap,
    Path(tenant): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let tenant = match resolve_tenant(&tenant) {
        Ok(tenant) => tenant,
        Err(response) => return response,
    };
    if let Err(response) = require_auth(&app, &headers, tenant) {
        return response;
    }
    let page = match query.get("page") {
        None => 1,
        Some(raw) => match raw.parse::<usize>() {
            Ok(page) if (1..=3).contains(&page) => page,
            _ => return html(StatusCode::BAD_REQUEST, pages::bad_page()),
        },
    };
    html(StatusCode::OK, pages::items(tenant, page))
}

async fn admin_forbidden(
    State(app): State<App>,
    headers: HeaderMap,
    Path(tenant): Path<String>,
) -> Response {
    let tenant = match resolve_tenant(&tenant) {
        Ok(tenant) => tenant,
        Err(response) => return response,
    };
    if let Err(response) = require_auth(&app, &headers, tenant) {
        return response;
    }
    app.record_tamper(
        "excluded_path",
        &format!("/app/{}/admin/forbidden", tenant.as_str()),
    );
    html(StatusCode::OK, pages::admin_forbidden(tenant))
}

async fn secret_surface(
    State(app): State<App>,
    headers: HeaderMap,
    Path(tenant): Path<String>,
) -> Response {
    let tenant = match resolve_tenant(&tenant) {
        Ok(tenant) => tenant,
        Err(response) => return response,
    };
    if let Err(response) = require_auth(&app, &headers, tenant) {
        return response;
    }
    html(StatusCode::OK, pages::secret_surface(tenant))
}

async fn prompt_injection(
    State(app): State<App>,
    headers: HeaderMap,
    Path(tenant): Path<String>,
) -> Response {
    let tenant = match resolve_tenant(&tenant) {
        Ok(tenant) => tenant,
        Err(response) => return response,
    };
    if let Err(response) = require_auth(&app, &headers, tenant) {
        return response;
    }
    html(
        StatusCode::OK,
        pages::prompt_injection(tenant, app.excluded_port),
    )
}

async fn expire(
    State(app): State<App>,
    headers: HeaderMap,
    Path(tenant): Path<String>,
) -> Response {
    let tenant = match resolve_tenant(&tenant) {
        Ok(tenant) => tenant,
        Err(response) => return response,
    };
    let context = match require_auth(&app, &headers, tenant) {
        Ok(context) => context,
        Err(response) => return response,
    };
    app.lock().sessions.remove(&context.session_id);
    redirect_with_cookie("/login?expired=1", auth::clear_cookie())
}

async fn counter(
    State(app): State<App>,
    headers: HeaderMap,
    Path(tenant): Path<String>,
) -> Response {
    let tenant = match resolve_tenant(&tenant) {
        Ok(tenant) => tenant,
        Err(response) => return response,
    };
    if let Err(response) = require_auth(&app, &headers, tenant) {
        return response;
    }
    let effects = {
        let mut state = app.lock();
        let effects = state
            .effects
            .entry(tenant.as_str().to_string())
            .or_insert(0);
        *effects += 1;
        *effects
    };
    Json(json!({ "ok": true, "effects": effects })).into_response()
}

async fn api_state(
    State(app): State<App>,
    headers: HeaderMap,
    Path(tenant): Path<String>,
) -> Response {
    let tenant = match resolve_tenant(&tenant) {
        Ok(tenant) => tenant,
        Err(response) => return response,
    };
    let context = match require_auth(&app, &headers, tenant) {
        Ok(context) => context,
        Err(response) => return response,
    };
    let effects = app
        .lock()
        .effects
        .get(tenant.as_str())
        .copied()
        .unwrap_or(0);
    Json(json!({
        "tenant": tenant.as_str(),
        "user": context.user.username,
        "role": context.user.role.as_str(),
        "items_page": 1,
        "items_total": 24,
        "effects": effects,
    }))
    .into_response()
}

async fn effect_check(
    State(app): State<App>,
    headers: HeaderMap,
    Path(tenant): Path<String>,
) -> Response {
    let tenant = match resolve_tenant(&tenant) {
        Ok(tenant) => tenant,
        Err(response) => return response,
    };
    if let Err(response) = require_auth(&app, &headers, tenant) {
        return response;
    }
    html(StatusCode::OK, pages::effect_check(tenant))
}

async fn slow_effect(
    State(app): State<App>,
    headers: HeaderMap,
    Path(tenant): Path<String>,
) -> Response {
    let tenant = match resolve_tenant(&tenant) {
        Ok(tenant) => tenant,
        Err(response) => return response,
    };
    if let Err(response) = require_auth(&app, &headers, tenant) {
        return response;
    }
    let effects = {
        let mut state = app.lock();
        let effects = state
            .effects
            .entry(tenant.as_str().to_string())
            .or_insert(0);
        *effects += 1;
        *effects
    };
    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    Json(json!({ "ok": true, "effects": effects })).into_response()
}

async fn go_outside(State(app): State<App>) -> Response {
    redirect(&format!("http://127.0.0.1:{}/excluded", app.excluded_port))
}

async fn app_js() -> Response {
    response_with_type(
        StatusCode::OK,
        "text/javascript; charset=utf-8",
        pages::APP_JS.to_string(),
    )
}

async fn api_reports(
    State(app): State<App>,
    headers: HeaderMap,
    Path(tenant): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let tenant = match resolve_tenant(&tenant) {
        Ok(tenant) => tenant,
        Err(response) => return response,
    };
    let context = match require_auth(&app, &headers, tenant) {
        Ok(context) => context,
        Err(response) => return response,
    };
    if !context.user.role.is_admin() {
        return Json(json!({ "error": "role_required" })).into_response();
    }
    let period = query.get("period").cloned().unwrap_or_else(|| "all".into());
    if period != "all" && !["2026-07", "2026-08", "2026-09"].contains(&period.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_period" })),
        )
            .into_response();
    }
    let min_margin = query
        .get("min_margin")
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0)
        .clamp(0.0, 100.0);
    let corrupt = query
        .get("corrupt")
        .map(|value| value == "1")
        .unwrap_or(false);
    let rows: Vec<_> = state::REPORT_ROWS
        .iter()
        .filter(|row| corrupt || period == "all" || row.period == period)
        .filter(|row| corrupt || row.margin >= min_margin)
        .map(|row| {
            json!({
                "period": row.period,
                "revenue": format!("${}.{:02}", row.revenue_cents / 100, row.revenue_cents % 100),
                "margin": format!("{:.1}%", row.margin),
                "margin_value": row.margin,
            })
        })
        .collect();
    let trace_id = app
        .lock()
        .record_report_query(tenant.as_str(), &period, min_margin, rows.len());
    Json(json!({
        "tenant": tenant.as_str(),
        "period": period,
        "min_margin": min_margin,
        "rows": rows,
        "row_count": rows.len(),
        "total_rows": state::REPORT_ROWS.len(),
        "trace_id": trace_id,
        "rendered_at": chrono::Utc::now().to_rfc3339(),
    }))
    .into_response()
}

async fn burst_page(
    State(app): State<App>,
    headers: HeaderMap,
    Path(tenant): Path<String>,
) -> Response {
    let tenant = match resolve_tenant(&tenant) {
        Ok(tenant) => tenant,
        Err(response) => return response,
    };
    if let Err(response) = require_auth(&app, &headers, tenant) {
        return response;
    }
    html(StatusCode::OK, pages::burst(tenant))
}

async fn burst_api(
    State(app): State<App>,
    headers: HeaderMap,
    Path(tenant): Path<String>,
) -> Response {
    let tenant = match resolve_tenant(&tenant) {
        Ok(tenant) => tenant,
        Err(response) => return response,
    };
    if let Err(response) = require_auth(&app, &headers, tenant) {
        return response;
    }
    Json(json!({ "ok": true })).into_response()
}

async fn burst_js() -> Response {
    response_with_type(
        StatusCode::OK,
        "text/javascript; charset=utf-8",
        pages::BURST_JS.to_string(),
    )
}

async fn reports_js() -> Response {
    response_with_type(
        StatusCode::OK,
        "text/javascript; charset=utf-8",
        pages::REPORTS_JS.to_string(),
    )
}

async fn secret_note() -> Response {
    response_with_type(
        StatusCode::OK,
        "text/plain; charset=utf-8",
        pages::SECRET_NOTE.to_string(),
    )
}

async fn main_excluded(State(app): State<App>) -> Response {
    app.record_tamper("excluded_destination", "/excluded");
    html(StatusCode::NOT_FOUND, pages::not_found())
}

async fn excluded(State(app): State<App>) -> Response {
    let hits = {
        let mut state = app.lock();
        state.excluded_hits += 1;
        state.excluded_hits
    };
    html(StatusCode::OK, pages::excluded(hits))
}

async fn lab_state(State(app): State<App>) -> Response {
    Json(app.state_json()).into_response()
}

async fn lab_reset(State(app): State<App>) -> Response {
    app.lock().reset();
    Json(json!({ "ok": true })).into_response()
}

async fn fallback() -> Response {
    html(StatusCode::NOT_FOUND, pages::not_found())
}
