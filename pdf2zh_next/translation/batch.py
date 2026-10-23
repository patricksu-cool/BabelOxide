"""CLI batch progress and failure counting, separate from process transport."""

from pdf2zh_next.config.model import SettingsModel


async def translate_batch(
    settings: SettingsModel,
    ignore_error: bool = False,
    *,
    config_type,
    progress_factory,
    stream_factory,
    translation_error,
    logger,
) -> int:
    rich_pbar_config = config_type(
        translator=None,
        lang_in=None,
        lang_out=None,
        input_file=None,
        font=None,
        pages=None,
        output_dir=None,
        doc_layout_model=1,
        use_rich_pbar=True,
    )
    progress_context, progress_handler = progress_factory(rich_pbar_config)
    input_files = settings.basic.input_files
    assert len(input_files) >= 1, "At least one input file is required"
    settings.basic.input_files = set()

    error_count = 0

    for file in input_files:
        logger.info(f"translate file: {file}")
        # 开始翻译
        with progress_context:
            try:
                async for event in stream_factory(settings, file):
                    progress_handler(event)
                    if settings.basic.debug:
                        logger.debug(event)
                    if event["type"] == "finish":
                        result = event["translate_result"]
                        logger.info("Translation Result:")
                        logger.info(f"  Original PDF: {result.original_pdf_path}")
                        logger.info(f"  Time Cost: {result.total_seconds:.2f}s")
                        logger.info(f"  Mono PDF: {result.mono_pdf_path or 'None'}")
                        logger.info(f"  Dual PDF: {result.dual_pdf_path or 'None'}")

                        token_usage = event.get("token_usage", {})
                        if token_usage:
                            logger.info("Token Usage:")
                            total_usage = {
                                "total": 0,
                                "prompt": 0,
                                "cache_hit_prompt": 0,
                                "completion": 0,
                            }
                            if "main" in token_usage:
                                main_usage = token_usage["main"]
                                logger.info(
                                    f"  Main Translator: Total {main_usage['total']}, Prompt {main_usage['prompt']}, Cache Hit Prompt {main_usage['cache_hit_prompt']}, Completion {main_usage['completion']}"
                                )
                                total_usage["total"] += main_usage["total"]
                                total_usage["prompt"] += main_usage["prompt"]
                                total_usage["cache_hit_prompt"] += main_usage[
                                    "cache_hit_prompt"
                                ]
                                total_usage["completion"] += main_usage["completion"]
                            if "term" in token_usage:
                                term_usage = token_usage["term"]
                                logger.info(
                                    f"  Term Translator: Total {term_usage['total']}, Prompt {term_usage['prompt']}, Cache Hit Prompt {term_usage['cache_hit_prompt']}, Completion {term_usage['completion']}"
                                )
                                total_usage["total"] += term_usage["total"]
                                total_usage["prompt"] += term_usage["prompt"]
                                total_usage["cache_hit_prompt"] += term_usage[
                                    "cache_hit_prompt"
                                ]
                                total_usage["completion"] += term_usage["completion"]
                            logger.info(
                                f"  Total Token Usage: Total {total_usage['total']}, Prompt {total_usage['prompt']}, Cache Hit Prompt {total_usage['cache_hit_prompt']}, Completion {total_usage['completion']}"
                            )
                        break
                    if event["type"] == "error":
                        error_msg = event.get("error", "Unknown error")
                        error_type = event.get("error_type", "UnknownError")
                        details = event.get("details", "")

                        logger.error(f"Error translating file {file}: {error_msg}")
                        logger.error(f"Error type: {error_type}")
                        if details:
                            logger.error(f"Error details: {details}")

                        error_count += 1
                        if not ignore_error:
                            raise RuntimeError(f"Translation error: {error_msg}")
                        break
            except translation_error as e:
                # Already logged in do_translate_async_stream
                error_count += 1
                if not ignore_error:
                    raise
            except Exception as e:
                logger.error(f"Error translating file {file}: {e}")
                error_count += 1
                if not ignore_error:
                    raise

    return error_count
