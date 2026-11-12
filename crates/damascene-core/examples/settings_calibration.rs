//! settings_calibration — Damascene fixture paired with the shadcn
//! settings/form reference.
//!
//! Run:
//! `cargo run -p damascene-core --example settings_calibration`

use damascene_core::prelude::*;

fn main() -> std::io::Result<()> {
    let viewport = Rect::new(0.0, 0.0, 1180.0, 780.0);
    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("out");

    let name = "settings_calibration";
    let theme = Theme::damascene_dark();
    let mut root = settings_calibration();
    let bundle = render_bundle_themed(&mut root, viewport, &theme);
    let written = write_bundle(&bundle, &out_dir, name)?;
    for p in &written {
        println!("wrote {}", p.display());
    }
    if !bundle.lint.findings.is_empty() {
        eprintln!(
            "\nlint findings for {name} ({}):",
            bundle.lint.findings.len()
        );
        eprint!("{}", bundle.lint.text());
    }

    Ok(())
}

fn settings_calibration() -> El {
    row([settings_sidebar(), settings_main()])
        .key("metric:root")
        .gap(0.0)
        .fill_size()
        .align(Align::Stretch)
        .fill(tokens::BACKGROUND)
}

fn settings_sidebar() -> El {
    column([
        row([
            icon_slot("settings"),
            column([
                text("Workspace")
                    .semibold()
                    .ellipsis()
                    .width(Size::Fill(1.0)),
                text("Settings").caption().ellipsis().width(Size::Fill(1.0)),
            ])
            .gap(2.0)
            .width(Size::Fill(1.0))
            .height(Size::Hug),
        ])
        .gap(tokens::SPACE_2)
        .height(Size::Fixed(44.0))
        .align(Align::Center),
        section_label("Personal"),
        side_item("users", "Profile", false),
        side_item("settings", "Account", true),
        side_item("alert-circle", "Security", false),
        side_item("bell", "Notifications", false),
        spacer().height(Size::Fixed(tokens::SPACE_4)),
        section_label("Workspace"),
        side_item("file-text", "Billing", false),
        side_item("bar-chart", "Appearance", false),
        side_item("activity", "Integrations", false),
        spacer(),
        column([text("Changes sync after save.").caption().wrap_text()])
            .padding(tokens::SPACE_2)
            .fill(tokens::MUTED)
            .radius(tokens::RADIUS_MD),
    ])
    .gap(tokens::SPACE_2)
    .padding(Sides::xy(tokens::SPACE_4, tokens::SPACE_2))
    .key("metric:sidebar")
    .width(Size::Fixed(244.0))
    .height(Size::Fill(1.0))
    .fill(tokens::CARD)
    .stroke(tokens::BORDER)
}

fn settings_main() -> El {
    column([
        settings_header(),
        row([settings_nav_card(), settings_body(), settings_aside()])
            .gap(tokens::SPACE_4)
            .padding(tokens::SPACE_4)
            .height(Size::Fill(1.0))
            .align(Align::Stretch),
    ])
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
}

fn settings_header() -> El {
    row([
        icon_button("menu").ghost(),
        divider().width(Size::Fixed(1.0)).height(Size::Fixed(22.0)),
        h3("Settings").key("metric:page.title"),
        spacer(),
        button("Reset").secondary(),
        button("Save changes").primary(),
    ])
    .key("metric:header")
    .gap(tokens::SPACE_3)
    .height(Size::Fixed(56.0))
    .padding(Sides::xy(tokens::SPACE_4, 0.0))
    .align(Align::Center)
    .stroke(tokens::BORDER)
}

fn settings_nav_card() -> El {
    column([
        settings_nav_item("Account", true),
        settings_nav_item("Security", false),
        settings_nav_item("Notifications", false),
        settings_nav_item("Appearance", false),
        settings_nav_item("Billing", false),
    ])
    .gap(tokens::SPACE_1)
    .padding(tokens::SPACE_1)
    .width(Size::Fixed(220.0))
    .height(Size::Fill(1.0))
    .style_profile(StyleProfile::Surface)
    .surface_role(SurfaceRole::Panel)
    .fill(tokens::CARD)
    .stroke(tokens::BORDER)
    .radius(tokens::RADIUS_MD)
    .shadow(tokens::SHADOW_MD)
}

fn settings_nav_item(label: &'static str, selected: bool) -> El {
    let mut item = row([
        El::new(Kind::Custom("nav-dot"))
            .fill(tokens::MUTED_FOREGROUND)
            .radius(tokens::RADIUS_PILL)
            .width(Size::Fixed(6.0))
            .height(Size::Fixed(6.0)),
        text(label)
            .font_weight(FontWeight::Medium)
            .ellipsis()
            .width(Size::Fill(1.0)),
    ])
    .key(if selected {
        "metric:settings.nav.row".to_string()
    } else {
        format!("settings-nav-{label}")
    })
    .metrics_role(MetricsRole::ListItem)
    .align(Align::Center)
    .focusable();

    if selected {
        item = item.current();
    } else {
        item = item.color(tokens::MUTED_FOREGROUND);
    }

    item
}

fn settings_body() -> El {
    column([
        column([
            h1("Account").heading().key("metric:section.title"),
            text("Manage identity, workspace defaults, and security preferences.")
                .muted()
                .wrap_text()
                .key("metric:page.subtitle"),
        ])
        .gap(tokens::SPACE_1)
        .height(Size::Hug),
        scroll([profile_card(), preferences_card()])
            .key("settings-body-scroll")
            .gap(tokens::SPACE_4)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0)),
    ])
    .gap(tokens::SPACE_4)
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
}

fn profile_card() -> El {
    card([
        card_header([
            card_title("Profile"),
            card_description("This information appears in audit logs and shared documents."),
        ]),
        card_content([form([
            row([
                setting_field("Display name", "Alicia Koch", "display-name"),
                setting_field("Email", "alicia@acme.co", "email"),
            ])
            .gap(tokens::SPACE_3),
            row([
                setting_select("Role", "Workspace admin", "role"),
                setting_select("Region", "US East", "region"),
            ])
            .gap(tokens::SPACE_3),
        ])]),
    ])
    .key("metric:profile.card")
}

fn setting_field(label: &'static str, value: &'static str, key: &'static str) -> El {
    form_item([
        form_label(label),
        form_control(
            text_input(key, value, &Selection::caret(key, value.len())).key(
                if key == "display-name" {
                    "metric:form.input"
                } else {
                    key
                },
            ),
        ),
    ])
    .width(Size::Fill(1.0))
}

fn setting_select(label: &'static str, value: &'static str, key: &'static str) -> El {
    form_item([form_label(label), form_control(select_trigger(key, value))]).width(Size::Fill(1.0))
}

fn preferences_card() -> El {
    card([
        card_header([
            card_title("Preferences"),
            card_description("Defaults used when creating new dashboards and exports."),
        ]),
        card_content([column([
            preference_row(
                "Compact navigation",
                "Use tighter rows in the sidebar and command menus.",
                switch("compact-navigation", true),
            ),
            divider(),
            preference_row(
                "Email summaries",
                "Send a daily digest when documents change.",
                switch("email-summaries", false),
            ),
            divider(),
            preference_row(
                "Require approval",
                "Route external sharing through an owner review.",
                checkbox("approval-required", true),
            ),
        ])
        .gap(0.0)
        .width(Size::Fill(1.0))])
        .padding(0.0),
    ])
    .key("metric:preferences.card")
}

fn preference_row(title: &'static str, description: &'static str, control: El) -> El {
    row([
        column([
            text(title).semibold().ellipsis().width(Size::Fill(1.0)),
            text(description)
                .caption()
                .ellipsis()
                .width(Size::Fill(1.0)),
        ])
        .gap(2.0)
        .width(Size::Fill(1.0))
        .height(Size::Hug),
        control,
    ])
    .key(if title == "Compact navigation" {
        "metric:preference.row".to_string()
    } else {
        format!("preference-{title}")
    })
    .metrics_role(MetricsRole::PreferenceRow)
    .gap(tokens::SPACE_4)
    .padding(Sides::xy(tokens::SPACE_4, tokens::SPACE_3))
    .align(Align::Center)
}

fn settings_aside() -> El {
    column([security_card(), scale_card()])
        .gap(tokens::SPACE_4)
        .width(Size::Fixed(300.0))
        .height(Size::Fill(1.0))
}

fn security_card() -> El {
    card([
        card_header([
            card_title("Security"),
            card_description("Two-factor authentication is enabled for all privileged users."),
        ]),
        card_content([
            compact_stat("Passkeys", "2 registered", badge("On").success()),
            compact_stat("Sessions", "3 active", button("Review").secondary()),
        ]),
    ])
    .width(Size::Fill(1.0))
}

fn scale_card() -> El {
    card([
        card_header([
            card_title("Interface scale"),
            card_description("Reference captures keep browser zoom fixed and vary root UI scale."),
        ]),
        card_content([
            row([text("Dense").caption(), spacer(), text("Default").caption()]),
            slider("interface-scale", 0.66).width(Size::Fill(1.0)),
        ]),
    ])
    .width(Size::Fill(1.0))
}

fn compact_stat(title: &'static str, detail: &'static str, control: El) -> El {
    row([
        column([
            text(title).semibold().ellipsis().width(Size::Fill(1.0)),
            text(detail).caption().ellipsis().width(Size::Fill(1.0)),
        ])
        .gap(2.0)
        .width(Size::Fill(1.0))
        .height(Size::Hug),
        control,
    ])
    .gap(tokens::SPACE_2)
    .height(Size::Fixed(44.0))
    .align(Align::Center)
}

fn section_label(label: &'static str) -> El {
    text(label)
        .caption()
        .height(Size::Fixed(22.0))
        .padding(Sides::xy(tokens::SPACE_2, 0.0))
}

fn side_item(icon_name: &'static str, label: &'static str, selected: bool) -> El {
    let mut item = row([
        icon(icon_name)
            .color(tokens::MUTED_FOREGROUND)
            .icon_size(tokens::ICON_SM)
            .width(Size::Fixed(tokens::ICON_SM)),
        text(label)
            .font_weight(FontWeight::Medium)
            .ellipsis()
            .width(Size::Fill(1.0)),
    ])
    .key(if selected {
        "metric:sidebar.nav.row".to_string()
    } else {
        format!("side-item-{label}")
    })
    .metrics_role(MetricsRole::ListItem)
    .gap(tokens::SPACE_2)
    .padding(Sides::xy(tokens::SPACE_2, 0.0))
    .height(Size::Fixed(32.0))
    .align(Align::Center)
    .focusable();

    if selected {
        item = item.current();
    } else {
        item = item.color(tokens::MUTED_FOREGROUND);
    }

    item
}

fn icon_slot(icon_name: &'static str) -> El {
    El::new(Kind::Custom("icon_cell"))
        .style_profile(StyleProfile::Surface)
        .child(
            icon(icon_name)
                .color(tokens::FOREGROUND)
                .icon_size(tokens::ICON_XS),
        )
        .align(Align::Center)
        .justify(Justify::Center)
        .fill(tokens::MUTED)
        .stroke(tokens::BORDER)
        .radius(tokens::RADIUS_SM)
        .width(Size::Fixed(30.0))
        .height(Size::Fixed(30.0))
}
