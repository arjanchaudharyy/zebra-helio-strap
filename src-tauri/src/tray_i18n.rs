//! Tray menu labels for each language (English for any language not listed).
//!
//! The native menu cannot use the frontend i18n, so it keeps its own copy here.
//! After the frontend picks the UI language, it sends the language code through
//! the `set_tray_locale` command. When the tray is built (before the frontend
//! loads), the system UI language is used as a first guess.
//!
//! The matching rules follow the frontend `detectLocale`: an exact match on the
//! full tag (`pt-BR`, `pt-PT`, `hi-IN`), otherwise the base language
//! (`de-AT` -> de). A bare `pt` gives European Portuguese. Anything unknown,
//! Chinese included, gives English.

/// The three tray menu labels.
pub struct TrayLabels {
    pub show: &'static str,
    pub sync: &'static str,
    pub quit: &'static str,
}

const EN: TrayLabels = TrayLabels {
    show: "Open ZeppBridge",
    sync: "Sync now",
    quit: "Quit",
};

const ES: TrayLabels = TrayLabels {
    show: "Abrir ZeppBridge",
    sync: "Sincronizar ahora",
    quit: "Salir",
};

const NL: TrayLabels = TrayLabels {
    show: "ZeppBridge openen",
    sync: "Nu synchroniseren",
    quit: "Afsluiten",
};

// Brazilian Portuguese drops the article ("Abrir ZeppBridge"). European Portuguese keeps it ("Abrir o ...").
const PT_BR: TrayLabels = TrayLabels {
    show: "Abrir ZeppBridge",
    sync: "Sincronizar agora",
    quit: "Sair",
};

const PT_PT: TrayLabels = TrayLabels {
    show: "Abrir o ZeppBridge",
    sync: "Sincronizar agora",
    quit: "Sair",
};

// Draft (no native-speaker review yet): de, ru and hi are first literal translations.
const DE: TrayLabels = TrayLabels {
    show: "ZeppBridge öffnen",
    sync: "Jetzt synchronisieren",
    quit: "Beenden",
};

const RU: TrayLabels = TrayLabels {
    show: "Открыть ZeppBridge",
    sync: "Синхронизировать",
    quit: "Выход",
};

const HI: TrayLabels = TrayLabels {
    show: "ZeppBridge खोलें",
    sync: "अभी सिंक करें",
    quit: "बंद करें",
};

const FR: TrayLabels = TrayLabels {
    show: "Ouvrir ZeppBridge",
    sync: "Synchroniser",
    quit: "Quitter",
};

const IT: TrayLabels = TrayLabels {
    show: "Apri ZeppBridge",
    sync: "Sincronizza ora",
    quit: "Esci",
};

/// Gets the tray labels for a language code. Accepts input such as `pt-BR`,
/// `de-AT` or `LANG=de_DE.UTF-8`: the code is lowercased and `_` becomes `-`.
/// An exact match comes first, then region subtags are cut one by one, then the
/// base language decides. Anything unknown gives English.
pub fn tray_labels(locale: &str) -> TrayLabels {
    let mut tag = locale.trim().to_ascii_lowercase().replace('_', "-");
    // An env value can carry a suffix, as in `de_DE.UTF-8`.
    if let Some(dot) = tag.find('.') {
        tag.truncate(dot);
    }
    while !tag.is_empty() {
        match tag.as_str() {
            "en" => return EN,
            "es" => return ES,
            "nl" => return NL,
            "pt-br" => return PT_BR,
            // A bare pt and unlisted Portuguese regions (pt-AO and others) give European Portuguese.
            "pt" | "pt-pt" => return PT_PT,
            "de" => return DE,
            "ru" => return RU,
            "hi" | "hi-in" => return HI,
            "fr" => return FR,
            "it" => return IT,
            _ => {}
        }
        match tag.rfind('-') {
            Some(cut) => tag.truncate(cut),
            None => break,
        }
    }
    EN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_regional_variants() {
        // pt-BR and pt-PT differ on purpose (the article). This proves the routing.
        assert_eq!(tray_labels("pt-BR").show, "Abrir ZeppBridge");
        assert_eq!(tray_labels("pt-PT").show, "Abrir o ZeppBridge");
        assert_eq!(tray_labels("hi-IN").quit, "बंद करें");
        assert_eq!(tray_labels("es").quit, "Salir");
        assert_eq!(tray_labels("it").quit, "Esci");
    }

    #[test]
    fn base_language_fallback() {
        assert_eq!(tray_labels("de-AT").quit, "Beenden");
        // Chinese locales are not supported. They fall back to English.
        for tag in ["zh", "zh-SG", "zh-Hans-HK", "zh-TW", "zh-Hant", "zh_TW.UTF-8"] {
            assert_eq!(tray_labels(tag).quit, "Quit", "{tag}");
        }
        // A bare pt and unlisted Portuguese regions give European Portuguese (with the article).
        assert_eq!(tray_labels("pt").show, "Abrir o ZeppBridge");
        assert_eq!(tray_labels("pt-AO").show, "Abrir o ZeppBridge");
        assert_eq!(tray_labels("en-GB").quit, "Quit");
        assert_eq!(tray_labels("fr-CA").quit, "Quitter");
    }

    #[test]
    fn env_style_and_unknown_fall_to_english() {
        assert_eq!(tray_labels("de_DE.UTF-8").quit, "Beenden");
        assert_eq!(tray_labels("ja-JP").quit, "Quit");
        assert_eq!(tray_labels("").quit, "Quit");
    }
}
