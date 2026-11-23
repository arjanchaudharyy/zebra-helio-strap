//! Text that we write in the login window: the window title and the top banner, by UI language.
//!
//! This text does not go through the frontend i18n layer (the login window loads the Zepp web page,
//! not our UI), so each language is here in place. Chinese is not supported and falls back to English. Normalization matches `tray_i18n::tray_labels`:
//! lowercase, `_` to `-`, drop the `.` suffix, exact match first, then cut region subtags one at a time. Unknown tags fall back to English.

/// Map the UI language to one of the languages that have text here. Bare `pt` and unlisted
/// Portuguese regions use European Portuguese, the same rule as tray_i18n.
fn login_locale_key(locale: &str) -> &'static str {
    let mut tag = locale.trim().to_ascii_lowercase().replace('_', "-");
    if let Some(dot) = tag.find('.') {
        tag.truncate(dot);
    }
    while !tag.is_empty() {
        match tag.as_str() {
            "es" => return "es",
            "nl" => return "nl",
            "pt-br" => return "pt-br",
            "pt" | "pt-pt" => return "pt",
            "de" => return "de",
            "ru" => return "ru",
            "hi" | "hi-in" => return "hi",
            "fr" => return "fr",
            "it" => return "it",
            "pl" => return "pl",
            "en" => return "en",
            _ => {}
        }
        match tag.rfind('-') {
            Some(cut) => tag.truncate(cut),
            None => break,
        }
    }
    "en"
}

/// Login window title.
pub(super) fn login_window_title(locale: &str) -> &'static str {
    match login_locale_key(locale) {
        "es" => "Iniciar sesión en Zepp",
        "nl" => "Inloggen bij Zepp",
        "pt-br" => "Entrar no Zepp",
        "pt" => "Iniciar sessão no Zepp",
        "de" => "Bei Zepp anmelden",
        "ru" => "Войти в Zepp",
        "hi" => "Zepp में साइन इन करें",
        "fr" => "Se connecter à Zepp",
        "it" => "Accedi a Zepp",
        "pl" => "Zaloguj się do Zepp",
        _ => "Sign in to Zepp",
    }
}

/// Banner at the top of the login window: only Zepp email / phone number + password is supported.
///
/// The second sentence states the real risk: if a Zepp account was registered with Google, a sign-in
/// with the same email and a password makes Zepp treat it as a different account. We do not describe how to
/// add a password in the Zepp App to such an account until we check the path on a real device.
pub(super) fn login_banner_text(locale: &str) -> &'static str {
    match login_locale_key(locale) {
        "es" => "ZeppBridge solo admite iniciar sesión con el correo o el teléfono de Zepp y contraseña. Google, Apple, WeChat, Xiaomi y Facebook no funcionan aquí. Si creaste tu cuenta de Zepp con uno de ellos, entrar con el mismo correo y una contraseña cuenta como otra cuenta de Zepp.",
        "nl" => "ZeppBridge ondersteunt alleen inloggen met je Zepp-e-mailadres of -telefoonnummer en wachtwoord. Inloggen met Google, Apple, WeChat, Xiaomi of Facebook werkt hier niet. Heb je je Zepp-account met een daarvan aangemaakt, dan telt inloggen met hetzelfde e-mailadres en een wachtwoord als een ander Zepp-account.",
        "pt-br" => "O ZeppBridge só aceita entrar com e-mail ou telefone da Zepp e senha. Login com Google, Apple, WeChat, Xiaomi ou Facebook não funciona aqui. Se você criou sua conta Zepp com um deles, entrar com o mesmo e-mail e uma senha conta como outra conta Zepp.",
        "pt" => "O ZeppBridge só suporta iniciar sessão com o e-mail ou o telemóvel da Zepp e palavra-passe. Google, Apple, WeChat, Xiaomi e Facebook não funcionam aqui. Se criou a sua conta Zepp com um deles, entrar com o mesmo e-mail e uma palavra-passe conta como outra conta Zepp.",
        "de" => "ZeppBridge unterstützt nur die Anmeldung mit Zepp-E-Mail-Adresse oder -Telefonnummer und Passwort. Google, Apple, WeChat, Xiaomi und Facebook funktionieren hier nicht. Wenn du dein Zepp-Konto mit einem davon erstellt hast, gilt die Anmeldung mit derselben E-Mail-Adresse und einem Passwort als anderes Zepp-Konto.",
        "ru" => "ZeppBridge поддерживает только вход по e-mail или номеру телефона Zepp и паролю. Вход через Google, Apple, WeChat, Xiaomi или Facebook здесь не работает. Если аккаунт Zepp создан через один из них, вход с тем же e-mail и паролем Zepp считает другим аккаунтом.",
        "hi" => "ZeppBridge केवल Zepp ईमेल या फ़ोन नंबर और पासवर्ड से साइन इन का समर्थन करता है। Google, Apple, WeChat, Xiaomi या Facebook से साइन इन यहाँ काम नहीं करता। अगर आपने अपना Zepp खाता इनमें से किसी से बनाया था, तो उसी ईमेल और पासवर्ड से साइन इन करना Zepp के लिए एक अलग खाता है।",
        "fr" => "ZeppBridge accepte uniquement la connexion avec l’e-mail ou le numéro de téléphone Zepp et un mot de passe. Google, Apple, WeChat, Xiaomi et Facebook ne fonctionnent pas ici. Si vous avez créé votre compte Zepp avec l’un d’eux, se connecter avec le même e-mail et un mot de passe correspond à un autre compte Zepp.",
        "it" => "ZeppBridge supporta solo l’accesso con e-mail o numero di telefono Zepp e password. Google, Apple, WeChat, Xiaomi e Facebook qui non funzionano. Se hai creato l’account Zepp con uno di questi, accedere con la stessa e-mail e una password conta come un altro account Zepp.",
        "pl" => "ZeppBridge obsługuje tylko logowanie adresem e-mail lub numerem telefonu Zepp i hasłem. Google, Apple, WeChat, Xiaomi i Facebook tu nie działają. Jeśli konto Zepp zostało założone przez jedną z tych usług, logowanie tym samym adresem e-mail i hasłem to dla Zepp inne konto.",
        _ => "ZeppBridge only supports signing in with your Zepp email or phone number and password. Google, Apple, WeChat, Xiaomi and Facebook sign-in don’t work here. If you created your Zepp account with one of those, signing in with the same email and a password counts as a different Zepp account.",
    }
}
