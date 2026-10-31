//! Language choices from the official v2.9.0 GUI. Selection is not a claim
//! that every provider accepts every code.
#[derive(Clone, Copy, Debug)]
pub struct Language {
    pub code: &'static str,
    pub english: &'static str,
    pub chinese: &'static str,
}
macro_rules! lang {
    ($code:literal, $en:literal, $zh:literal) => {
        Language {
            code: $code,
            english: $en,
            chinese: $zh,
        }
    };
}
pub const CATALOG: &[Language] = &[
    lang!("en", "English", "英文"),
    lang!("zh-CN", "Simplified Chinese", "簡體中文"),
    lang!(
        "zh-HK",
        "Traditional Chinese - Hong Kong",
        "繁體中文（香港）"
    ),
    lang!("zh-TW", "Traditional Chinese - Taiwan", "繁體中文（台灣）"),
    lang!("ja", "Japanese", "日文"),
    lang!("ko", "Korean", "韓文"),
    lang!("pl", "Polish", "波蘭文"),
    lang!("ru", "Russian", "俄文"),
    lang!("es", "Spanish", "西班牙文"),
    lang!("pt", "Portuguese", "葡萄牙文"),
    lang!("pt-BR", "Brazilian Portuguese", "葡萄牙文（巴西）"),
    lang!("fr", "French", "法文"),
    lang!("ms", "Malay", "馬來文"),
    lang!("id", "Indonesian", "印尼文"),
    lang!("tk", "Turkmen", "土庫曼文"),
    lang!("tl", "Filipino (Tagalog)", "菲律賓文"),
    lang!("vi", "Vietnamese", "越南文"),
    lang!("kk", "Kazakh (Latin)", "哈薩克文（拉丁字母）"),
    lang!("de", "German", "德文"),
    lang!("nl", "Dutch", "荷蘭文"),
    lang!("ga", "Irish", "愛爾蘭文"),
    lang!("it", "Italian", "義大利文"),
    lang!("el", "Greek", "希臘文"),
    lang!("sv", "Swedish", "瑞典文"),
    lang!("da", "Danish", "丹麥文"),
    lang!("no", "Norwegian", "挪威文"),
    lang!("is", "Icelandic", "冰島文"),
    lang!("fi", "Finnish", "芬蘭文"),
    lang!("uk", "Ukrainian", "烏克蘭文"),
    lang!("cs", "Czech", "捷克文"),
    lang!("ro", "Romanian", "羅馬尼亞文"),
    lang!("hu", "Hungarian", "匈牙利文"),
    lang!("sk", "Slovak", "斯洛伐克文"),
    lang!("hr", "Croatian", "克羅埃西亞文"),
    lang!("et", "Estonian", "愛沙尼亞文"),
    lang!("lv", "Latvian", "拉脫維亞文"),
    lang!("lt", "Lithuanian", "立陶宛文"),
    lang!("be", "Belarusian", "白俄羅斯文"),
    lang!("mk", "Macedonian", "馬其頓文"),
    lang!("sq", "Albanian", "阿爾巴尼亞文"),
    lang!("sr", "Serbian (Cyrillic)", "塞爾維亞文（西里爾字母）"),
    lang!("sl", "Slovenian", "斯洛維尼亞文"),
    lang!("ca", "Catalan", "加泰隆尼亞文"),
    lang!("bg", "Bulgarian", "保加利亞文"),
    lang!("mt", "Maltese", "馬爾他文"),
    lang!("sw", "Swahili", "史瓦希里文"),
    lang!("am", "Amharic", "阿姆哈拉文"),
    lang!("om", "Oromo", "奧羅莫文"),
    lang!("ti", "Tigrinya", "提格利尼亞文"),
    lang!("ht", "Haitian Creole", "海地克里奧爾文"),
    lang!("la", "Latin", "拉丁文"),
    lang!("lo", "Lao", "寮文"),
    lang!("ml", "Malayalam", "馬拉雅拉姆文"),
    lang!("gu", "Gujarati", "古吉拉特文"),
    lang!("th", "Thai", "泰文"),
    lang!("my", "Burmese", "緬甸文"),
    lang!("ta", "Tamil", "坦米爾文"),
    lang!("te", "Telugu", "泰盧固文"),
    lang!("or", "Oriya", "奧里亞文"),
    lang!("hy", "Armenian", "亞美尼亞文"),
    lang!("mn", "Mongolian (Cyrillic)", "蒙古文（西里爾字母）"),
    lang!("ka", "Georgian", "喬治亞文"),
    lang!("km", "Khmer", "高棉文"),
    lang!("bs", "Bosnian", "波士尼亞文"),
    lang!("lb", "Luxembourgish", "盧森堡文"),
    lang!("rm", "Romansh", "羅曼什文"),
    lang!("tr", "Turkish", "土耳其文"),
    lang!("si", "Sinhala", "僧伽羅文"),
    lang!("uz", "Uzbek", "烏茲別克文"),
    lang!("ky", "Kyrgyz", "吉爾吉斯文"),
    lang!("tg", "Tajik", "塔吉克文"),
    lang!("ab", "Abkhazian", "阿布哈茲文"),
    lang!("aa", "Afar", "阿法爾文"),
    lang!("af", "Afrikaans", "南非荷蘭文"),
    lang!("ak", "Akan", "阿坎文"),
    lang!("an", "Aragonese", "亞拉岡文"),
    lang!("av", "Avaric", "阿瓦爾文"),
    lang!("ee", "Ewe", "埃維文"),
    lang!("ay", "Aymara", "艾馬拉文"),
    lang!("oj", "Ojibwa", "奧吉布瓦文"),
    lang!("oc", "Occitan", "奧克西坦文"),
    lang!("os", "Ossetian", "奧塞梯文"),
    lang!("pi", "Pali", "巴利文"),
    lang!("ba", "Bashkir", "巴什基爾文"),
    lang!("eu", "Basque", "巴斯克文"),
    lang!("br", "Breton", "布列塔尼文"),
    lang!("ch", "Chamorro", "查莫羅文"),
    lang!("ce", "Chechen", "車臣文"),
    lang!("cv", "Chuvash", "楚瓦什文"),
    lang!("tn", "Tswana", "茨瓦納文"),
    lang!("nr", "Ndebele, South", "南恩德貝萊文"),
    lang!("ng", "Ndonga", "恩東加文"),
    lang!("fo", "Faroese", "法羅文"),
    lang!("fj", "Fijian", "斐濟文"),
    lang!("fy", "Frisian, Western", "西弗里斯蘭文"),
    lang!("lg", "Ganda", "盧干達文"),
    lang!("kg", "Kongo", "剛果文"),
    lang!("kl", "Kalaallisut", "格陵蘭文"),
    lang!("cu", "Church Slavic", "教會斯拉夫文"),
    lang!("gn", "Guarani", "瓜拉尼文"),
    lang!("ia", "Interlingua", "國際語"),
    lang!("hz", "Herero", "赫雷羅文"),
    lang!("ki", "Kikuyu", "基庫尤文"),
    lang!("rn", "Rundi", "隆迪文"),
    lang!("rw", "Kinyarwanda", "盧安達文"),
    lang!("gl", "Galician", "加利西亞文"),
    lang!("kr", "Kanuri", "卡努里文"),
    lang!("kw", "Cornish", "康瓦爾文"),
    lang!("kv", "Komi", "科米文"),
    lang!("xh", "Xhosa", "科薩文"),
    lang!("co", "Corsican", "科西嘉文"),
    lang!("cr", "Cree", "克里文"),
    lang!("qu", "Quechua", "克丘亞文"),
    lang!("ku", "Kurdish (Latin)", "庫德文（拉丁字母）"),
    lang!("kj", "Kuanyama", "寬亞瑪文"),
    lang!("li", "Limburgan", "林堡文"),
    lang!("ln", "Lingala", "林加拉文"),
    lang!("gv", "Manx", "曼島文"),
    lang!("mg", "Malagasy", "馬達加斯加文"),
    lang!("mh", "Marshallese", "馬紹爾文"),
    lang!("mi", "Maori", "毛利文"),
    lang!("nv", "Navajo", "納瓦霍文"),
    lang!("na", "Nauru", "諾魯文"),
    lang!("ny", "Nyanja", "齊切瓦文"),
    lang!("nn", "Norwegian Nynorsk", "新挪威文"),
    lang!("sc", "Sardinian", "薩丁尼亞文"),
    lang!("se", "Northern Sami", "北薩米文"),
    lang!("sm", "Samoan", "薩摩亞文"),
    lang!("sg", "Sango", "桑戈文"),
    lang!("sn", "Shona", "紹納文"),
    lang!("eo", "Esperanto", "世界語"),
    lang!("gd", "Scottish Gaelic", "蘇格蘭蓋爾文"),
    lang!("so", "Somali", "索馬利文"),
    lang!("st", "Southern Sotho", "南索托文"),
    lang!("tt", "Tatar", "韃靼文"),
    lang!("ty", "Tahitian", "大溪地文"),
    lang!("to", "Tongan", "東加文"),
    lang!("tw", "Twi", "契維文"),
    lang!("wa", "Walloon", "瓦隆文"),
    lang!("cy", "Welsh", "威爾斯文"),
    lang!("ve", "Venda", "文達文"),
    lang!("vo", "Volapük", "沃拉普克語"),
    lang!("ie", "Interlingue", "西方國際語"),
    lang!("ho", "Hiri Motu", "希里莫圖文"),
    lang!("ig", "Igbo", "伊博文"),
    lang!("io", "Ido", "伊多語"),
    lang!("iu", "Inuktitut", "因紐特文"),
    lang!("ik", "Inupiaq", "依努皮維克文"),
    lang!("ii", "Sichuan Yi", "四川彝文"),
    lang!("yo", "Yoruba", "約魯巴文"),
    lang!("za", "Zhuang", "壯文"),
    lang!("ts", "Tsonga", "宗加文"),
    lang!("zu", "Zulu", "祖魯文"),
];

pub fn display(code: &str, chinese: bool) -> String {
    match CATALOG.iter().find(|language| language.code == code) {
        Some(language) if chinese => format!("{}（{}）", language.chinese, language.english),
        Some(language) => format!("{} ({})", language.english, language.code),
        None => code.to_owned(),
    }
}
/// None means the code has not been verified for this provider.
pub fn supported(engine: &str, code: &str, source: bool) -> Option<bool> {
    match engine.to_ascii_lowercase().as_str() {
        "qwenmt" | "qwen-mt" => Some(
            [
                "zh-CN", "zh-TW", "en", "fr", "de", "ja", "ko", "ru", "es", "it",
            ]
            .contains(&code),
        ),
        "deepl" => {
            const COMMON: &[&str] = &[
                "ar", "bg", "cs", "da", "de", "el", "es", "et", "fi", "fr", "he", "hu", "id", "it",
                "ja", "ko", "lt", "lv", "nb", "nl", "pl", "ro", "ru", "sk", "sl", "sv", "th", "tr",
                "uk", "vi",
            ];
            let code = code.to_ascii_lowercase();
            Some(
                COMMON.contains(&code.as_str())
                    || if source {
                        [
                            "auto", "zh", "zh-cn", "zh-tw", "zh-hk", "pt", "pt-br", "pt-pt", "no",
                            "en",
                        ]
                        .contains(&code.as_str())
                    } else {
                        [
                            "en", "en-us", "en-gb", "zh", "zh-cn", "zh-hans", "zh-tw", "zh-hk",
                            "zh-hant", "pt", "pt-pt", "pt-br", "es-419", "no",
                        ]
                        .contains(&code.as_str())
                    },
            )
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn official_catalog_preserves_codes_and_custom() {
        assert_eq!(CATALOG.len(), 153);
        assert!(CATALOG.iter().any(|x| x.code == "zh-HK"));
        assert!(!CATALOG.iter().any(|x| x.code == "zh-TC"));
        assert_eq!(display("private-code", true), "private-code");
    }
    #[test]
    fn provider_limits_match_core() {
        assert_eq!(supported("QwenMt", "zh-HK", true), Some(false));
        assert_eq!(supported("QwenMt", "zh-TW", false), Some(true));
        assert_eq!(supported("DeepL", "auto", true), Some(true));
        assert_eq!(supported("DeepL", "auto", false), Some(false));
        assert_eq!(supported("Google", "zh-HK", true), None);
    }
}
