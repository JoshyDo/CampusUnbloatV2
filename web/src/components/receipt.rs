use campusunbloat_shared::{ExamEntry, ExamOffer};
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlAnchorElement, HtmlCanvasElement};

#[derive(Clone, PartialEq)]
pub struct Receipt {
    pub title: String,
    pub filename: String,
    pub rows: Vec<(String, String)>,
}

pub fn build(exam: &ExamEntry, offer: &ExamOffer, registered: bool, message: &str) -> Receipt {
    let now = chrono::Local::now();
    let title = if registered {
        "Prüfungsanmeldung"
    } else {
        "Prüfungsabmeldung"
    };
    let rows = [
        ("Prüfung", exam.name.clone()),
        ("Termin", format!("{} {}", offer.date, offer.time)),
        ("Raum", offer.room.clone()),
        ("Prüfungsart", offer.mode.clone()),
        ("Angebotsnummer", offer.offer_no.clone()),
        ("Zeitpunkt", now.format("%d.%m.%Y %H:%M:%S").to_string()),
        ("Campus Dual", message.to_string()),
    ]
    .into_iter()
    .filter(|(_, value)| !value.trim().is_empty())
    .map(|(key, value)| (key.to_string(), value))
    .collect();
    Receipt {
        title: title.to_string(),
        filename: format!(
            "{}-{}.png",
            if registered { "anmeldung" } else { "abmeldung" },
            now.format("%Y%m%d-%H%M%S")
        ),
        rows,
    }
}

/// Splits text into lines that fit the given pixel width.
fn wrap(context: &CanvasRenderingContext2d, text: &str, width: f64) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        let fits = context
            .measure_text(&candidate)
            .map(|metrics| metrics.width() <= width)
            .unwrap_or(true);
        if fits || current.is_empty() {
            current = candidate;
        } else {
            lines.push(std::mem::replace(&mut current, word.to_string()));
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Renders the receipt onto a canvas and saves it as a PNG.
pub fn download(receipt: &Receipt) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let Ok(element) = document.create_element("canvas") else {
        return;
    };
    let Ok(canvas) = element.dyn_into::<HtmlCanvasElement>() else {
        return;
    };
    const WIDTH: f64 = 900.0;
    const LABEL_X: f64 = 40.0;
    const VALUE_X: f64 = 260.0;
    const LINE: f64 = 34.0;
    canvas.set_width(WIDTH as u32);
    canvas.set_height(100);
    let Some(context) = canvas
        .get_context("2d")
        .ok()
        .flatten()
        .and_then(|context| context.dyn_into::<CanvasRenderingContext2d>().ok())
    else {
        return;
    };

    context.set_font("26px system-ui, sans-serif");
    let wrapped: Vec<(&str, Vec<String>)> = receipt
        .rows
        .iter()
        .map(|(key, value)| (key.as_str(), wrap(&context, value, WIDTH - VALUE_X - 40.0)))
        .collect();
    let body_lines: usize = wrapped.iter().map(|(_, lines)| lines.len()).sum();
    let height = 170.0 + body_lines as f64 * LINE + wrapped.len() as f64 * 12.0 + 70.0;
    // Resizing resets the context state, so all drawing happens after this.
    canvas.set_height(height as u32);

    context.set_fill_style_str("#ffffff");
    context.fill_rect(0.0, 0.0, WIDTH, height);
    context.set_fill_style_str("#111827");
    context.set_font("bold 38px system-ui, sans-serif");
    let _ = context.fill_text(&receipt.title, LABEL_X, 70.0);
    context.set_fill_style_str("#16a34a");
    context.fill_rect(LABEL_X, 90.0, WIDTH - 2.0 * LABEL_X, 4.0);

    let mut y = 150.0;
    for (key, lines) in &wrapped {
        context.set_fill_style_str("#6b7280");
        context.set_font("24px system-ui, sans-serif");
        let _ = context.fill_text(key, LABEL_X, y);
        context.set_fill_style_str("#111827");
        context.set_font("26px system-ui, sans-serif");
        for line in lines {
            let _ = context.fill_text(line, VALUE_X, y);
            y += LINE;
        }
        y += 12.0;
    }
    context.set_fill_style_str("#9ca3af");
    context.set_font("18px system-ui, sans-serif");
    let _ = context.fill_text(
        "Erstellt von CampusUnbloatV2. Kein offizieller Nachweis der Hochschule.",
        LABEL_X,
        height - 30.0,
    );

    let Ok(url) = canvas.to_data_url() else {
        return;
    };
    if let Ok(element) = document.create_element("a")
        && let Ok(anchor) = element.dyn_into::<HtmlAnchorElement>()
    {
        anchor.set_href(&url);
        anchor.set_download(&receipt.filename);
        anchor.click();
    }
}
