"""Token accounting for the BabelDOC finish event."""


def _translator_token_usage(translator) -> dict[str, int]:
    fields = (
        ("total", "token_count"),
        ("prompt", "prompt_token_count"),
        ("completion", "completion_token_count"),
        ("cache_hit_prompt", "cache_hit_prompt_token_count"),
    )
    return {
        name: getattr(translator, attribute).value
        if hasattr(translator, attribute)
        else 0
        for name, attribute in fields
    }


def collect_token_usage(config) -> dict[str, dict[str, int]]:
    """Keep separate counters and the existing shared-translator subtraction rule."""
    token_usage = {}
    if hasattr(config.translator, "token_count"):
        token_usage["main"] = _translator_token_usage(config.translator)

    if (
        hasattr(config.term_extraction_translator, "token_count")
        and config.term_extraction_translator != config.translator
    ):
        token_usage["term"] = _translator_token_usage(config.term_extraction_translator)
    elif config.term_extraction_token_usage:
        token_usage["term"] = {
            "total": config.term_extraction_token_usage["total_tokens"],
            "prompt": config.term_extraction_token_usage["prompt_tokens"],
            "completion": config.term_extraction_token_usage["completion_tokens"],
            "cache_hit_prompt": config.term_extraction_token_usage[
                "cache_hit_prompt_tokens"
            ],
        }
        if sum(token_usage["term"].values()) == 0:
            token_usage.pop("term")
    if (
        "main" in token_usage
        and "term" in token_usage
        and config.term_extraction_translator
        and config.term_extraction_translator == config.translator
    ):
        term_usage = token_usage["term"]
        main_usage = token_usage["main"]
        for field in ("total", "prompt", "completion", "cache_hit_prompt"):
            main_usage[field] -= term_usage[field]
    return token_usage
