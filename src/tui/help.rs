use super::{
    app::{App, View},
    components,
    explanations::{self, Metric},
    overview::{self, Target},
    ui::heading,
};
use ratatui::{layout::Rect, text::Line, widgets::Clear, Frame};

pub(super) fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let popup = Rect::new(
        area.x + 2,
        area.y + 1,
        area.width.saturating_sub(4),
        area.height.saturating_sub(2),
    );
    frame.render_widget(Clear, popup);
    let selected = if app.view == View::Overview {
        overview::selected(app)
    } else {
        None
    };
    let topics = match selected {
        Some(Target::Metric(Metric::Status)) => vec![Metric::Status],
        Some(Target::Metric(m)) => vec![m, Metric::Status],
        Some(Target::Longest(_) | Target::Deepest(_)) => {
            vec![Metric::PhysicalLoc, Metric::Depth, Metric::FlowNodes]
        }
        Some(
            Target::Hotspot(_) | Target::Historical(_) | Target::Group(overview::Group::Hotspots),
        ) => vec![
            Metric::Changes,
            Metric::Churn,
            Metric::Candidates,
            Metric::CoChange,
        ],
        _ => match app.view {
            View::Overview => vec![
                Metric::SourceLoc,
                Metric::Functions,
                Metric::Length,
                Metric::Depth,
                Metric::Candidates,
                Metric::Status,
            ],
            View::Map => vec![
                Metric::Coverage,
                Metric::Verified,
                Metric::Unclassified,
                Metric::Depth,
                Metric::Status,
            ],
            View::Calls => vec![Metric::Calls, Metric::Status],
            _ => vec![
                Metric::Calls,
                Metric::PhysicalLoc,
                Metric::Functions,
                Metric::Status,
            ],
        },
    };
    let mut lines = vec![heading("Explore this view"), Line::raw(match app.view {
        View::Overview => "Choose a task · Enter explore · Tab details. Technical groups expand with Enter.",
        View::Map => "Crate: m modules. Module: Enter focus · o source · p review · v relationships · Esc back.",
        View::Calls => "j/k selection · Enter focus · g Graph/List · x expand direct neighbor · o source · Esc back.",
        View::Symbols => "/ search Rust symbol names · Enter inspect exact symbol · c calls · Esc back.",
        View::Explorer => "h/l tree · Enter file or selected RA symbol · c calls in Inspector · Esc file / origin.",
    }), Line::raw("1–5 views · Tab / Shift+Tab panes · Ctrl+h/l panes · q quit"),
        heading("Interpret measured data · j/k scroll · ? / Esc close")];
    for topic in topics {
        lines.extend(explanations::lines(topic));
        lines.push(Line::raw(""));
    }
    frame.render_widget(
        components::scrollable(
            lines,
            components::styled_panel(" CONTEXT HELP · j/k scroll · Esc close ", true),
            popup,
            app.help_scroll,
            &app.help_scroll_limit,
        ),
        popup,
    );
}
