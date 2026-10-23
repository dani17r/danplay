# Las pruebas de Python, pasadas a Rust

Lo que queda de Python (`danplay/` y `tests/`) solo sirve de testigo: antes
de borrarlo, cada prueba de `tests/` tiene que estar cubierta en Rust, o
apuntado por qué no hace falta. Esta es la cuenta, archivo por archivo: cada
prueba de Python → la prueba de Rust que la cubre (ruta y nombre), «ya
estaba en …» si se pasó en las fases 1 a 3, o «no se pasa: …» con el porqué.

Las de la API (`tests/test_api.py` y compañía) van en `server/src/api_tests/`
y se cuentan aparte. Aquí, las que prueban la biblioteca por dentro: lo que
ahora vive en `library`, `analysis` y `engine` (y, cuando lo que hacía
Python ahora lo hace un servicio del servidor, en `server/src/services/`).

Una prueba con `parametrize` es una sola fila: en Rust va con los mismos
casos en una tabla.

Convenciones de las de Rust:

- Audio de verdad, con ffmpeg en una carpeta temporal propia que se borra al
  acabar (`library/src/testkit.rs`, `server/src/testkit.rs`); sin ffmpeg, la
  prueba lo dice y vuelve sin comprobar nada.
- Nada de Python: los programas de mentira son guiones de sh.
- Lo que en Python se comparaba con una biblioteca de Python (python-dotenv,
  rapidfuzz, Hypothesis…) se compara con lo que daba esa biblioteca: casos
  generados una vez y guardados junto a la prueba, o la misma cuenta escrita
  en la prueba.

## tests/test_core.py

| Prueba de Python | En Rust |
|---|---|
| `test_strip_accents_keeps_enye` | `library/src/names/tests.rs` `strip_accents_keeps_enye` |
| `test_capitalize_words` | `library/src/names/tests.rs` `capitalize_words_leaves_mixed_case_alone` |
| `test_clean_strips_noise` | `library/src/names/tests.rs` `clean_strips_noise` |
| `test_sanitize_removes_control_characters` | `library/src/names/tests.rs` `sanitize_removes_control_characters` |
| `test_nuls_do_not_turn_into_enye` | `library/src/names/tests.rs` `nuls_do_not_turn_into_enye` |
| `test_artist_dash_title_is_read_from_the_name` | `library/src/names/tests.rs` `artist_dash_title_is_read_from_the_name` |
| `test_without_dash_there_is_no_artist` | `library/src/names/tests.rs` `without_dash_there_is_no_artist` |
| `test_feat_does_not_eat_titles_starting_with_con` | `library/src/names/tests.rs` `feat_does_not_eat_titles_starting_with_con` |
| `test_extract_feat` | `library/src/names/tests.rs` `extract_feat_cases` |
| `test_final_name` | `library/src/names/tests.rs` `final_name_with_and_without_feat` |
| `test_name_without_invalid_chars` | `library/src/names/tests.rs` `name_without_invalid_chars` |
| `test_duplicate_suffix` | `library/src/names/tests.rs` `free_name_adds_the_duplicate_suffix` |
| `test_match_key_groups_variants` | `library/src/names/tests.rs` `match_key_groups_variants` |
| `test_artist_detected_from_vocabulary` | `library/src/names/tests.rs` `artist_detected_from_vocabulary` |
| `test_ambiguous_detection_lowers_confidence` | `library/src/names/tests.rs` `ambiguous_detection_lowers_confidence` |
| `test_transposition` | `library/src/theory.rs` `transposition_between_keys` |
| `test_transposition_full_cycle` | `library/src/theory.rs` `transposition_full_cycle` |
| `test_distance_between_keys` | `library/src/theory.rs` `distance_between_keys` |
| `test_latin_notation` | `library/src/theory.rs` `latin_notation` |
| `test_suggested_capo` | ya estaba en `library/src/theory.rs` `latin_names_and_capo` (Sib con formas de Sol = capo 3, y las demás) |
| `test_identical_finds_exact_copies` | ya estaba en `library/src/duplicates/tests.rs` `byte_identical_files` |
| `test_rust_is_active` | no se pasa: comprueba que Python tenga cargado el módulo de PyO3 |
| `test_rust_and_python_hashes_match` | `library/src/duplicates/tests.rs` `parallel_and_single_hashes_match` (los hashes en paralelo, el de un archivo y el md5 de siempre) |
| `test_tags_round_trip` | `library/src/tags/tests.rs` `tags_round_trip` |
| `test_zero_stars_read_as_zero` | `library/src/tags/tests.rs` `zero_stars_read_as_zero` |
| `test_extract_cover` | `library/src/tags/tests.rs` `extract_cover` |
| `test_wav_duration_is_not_zero` | `library/src/tags/tests.rs` `wav_duration_is_not_zero` (con un wav sintético: la música de quien prueba no se mira) |
| `test_mp3_duration_still_right` | `library/src/tags/tests.rs` `mp3_duration_still_right` |
| `test_strip_duplicate_suffix` | `library/src/duplicates/tests.rs` `strip_duplicate_suffix` |
| `test_resolve_deletes_others_and_renames` | `server/src/services/duplicates.rs` `resolve_deletes_others_and_renames` |
| `test_resolve_keeps_clean_one_unrenamed` | `server/src/services/duplicates.rs` `resolve_keeps_clean_one_unrenamed` |
| `test_resolve_deletes_nothing_on_dry_run` | `server/src/services/duplicates.rs` `resolve_deletes_nothing_on_dry_run` |
| `test_resolve_is_dry_by_default` | `server/src/services/duplicates.rs` `resolve_is_dry_by_default` (en Rust no hay valor por defecto: lo que llega de la API sin `dry_run` es `true`) |
| `test_resolve_refuses_paths_outside_the_library` | `server/src/services/duplicates.rs` `resolve_refuses_paths_outside_the_library` |
| `test_resolve_refuses_non_audio` | `server/src/services/duplicates.rs` `resolve_refuses_non_audio` |
| `test_resolve_warns_if_kept_is_missing` | `server/src/services/duplicates.rs` `resolve_warns_if_kept_is_missing` |
| `test_recognizes_youtube_urls` | `library/src/youtube/tests.rs` `recognizes_youtube_urls` |
| `test_plain_text_becomes_a_search` | `library/src/youtube/tests.rs` `plain_text_becomes_a_search` |
| `test_a_url_is_kept_as_is` | `library/src/youtube/tests.rs` `a_url_is_kept_as_is` |
| `test_provisional_name_is_already_clean` | `library/src/youtube/tests.rs` `provisional_name_is_already_clean` |
| `test_falls_back_to_id_without_title` | `library/src/youtube/tests.rs` `falls_back_to_id_without_title` |
| `test_qualities_match_the_app` | `library/src/youtube/tests.rs` `qualities_match_the_app` |
| `test_never_tags_with_the_channel_name` | `library/src/youtube/tests.rs` `never_tags_with_the_channel_name` (lo que decide `_tag_if_trustworthy` es `trusted_tags`) |
| `test_download_state_starts_idle` | `server/src/services/youtube/tests.rs` `the_download_state_starts_idle` |
| `test_only_one_can_hold_the_download_turn` | `server/src/services/youtube/tests.rs` `only_one_can_hold_the_download_turn` |
| `test_run_job_does_not_block_when_the_turn_is_already_held` | `server/src/services/youtube/tests.rs` `run_job_does_not_block_when_the_turn_is_already_held` (con una descarga de mentira, `run_many_with`; el mismo fallo, por la API y con un yt-dlp de mentira, ya estaba en `server/src/api_tests/downloads.rs` `the_download_button_downloads_and_releases_the_lock`) |
| `test_run_many_uses_one_turn_and_publishes_progress` | `server/src/services/youtube/tests.rs` `run_many_uses_one_turn_and_publishes_progress` |
| `test_run_many_with_nothing_releases_the_turn` | `server/src/services/youtube/tests.rs` `run_many_with_nothing_releases_the_turn` |
| `test_a_failed_download_does_not_leave_the_turn_held` | `server/src/services/youtube/tests.rs` `a_failed_download_does_not_leave_the_turn_held` |
| `test_cover_queries_go_from_precise_to_loose` | ya estaba en `library/src/enrich/tests.rs` `cover_queries_go_from_precise_to_loose` |
| `test_without_artist_it_searches_by_title_only` | `library/src/enrich/tests.rs` `without_artist_it_searches_by_title_only` |
| `test_the_album_takes_precedence_over_the_title` | ya estaba en `library/src/enrich/tests.rs` `cover_queries_go_from_precise_to_loose` |
| `test_an_empty_title_makes_no_junk_queries` | ya estaba en `library/src/enrich/tests.rs` `cover_queries_go_from_precise_to_loose` |
| `test_what_the_ai_does_not_know_is_not_saved` | `library/src/enrich/tests.rs` `what_the_ai_does_not_know_is_not_saved` |
| `test_the_year_is_four_digits_or_nothing` | `library/src/enrich/tests.rs` `the_year_is_four_digits_or_nothing` |
| `test_a_real_value_is_saved` | `library/src/enrich/tests.rs` `a_real_value_is_saved` |
| `test_the_shortcut_finds_exactly_the_same_duplicate_groups` | `library/src/duplicates/tests.rs` `the_shortcut_finds_exactly_the_same_duplicate_groups` (la referencia, todos contra todos con `token_set_ratio`; los 400 nombres al azar, con un generador fijo y sin repetir ruta) |
| `test_identical_files_are_grouped_byte_by_byte` | ya estaba en `library/src/duplicates/tests.rs` `byte_identical_files` |
| `test_all_the_pieces_carry_the_same_version` | `library/src/repo_checks.rs` `all_the_pieces_carry_the_same_version` (sin danplay/__init__.py, que se va con Python) |
| `test_the_version_is_not_repeated` | `library/src/repo_checks.rs` `the_version_is_not_repeated` (todos los miembros del workspace; core/pyproject.toml no: es de Python) |
| `test_the_version_script_touches_every_place_this_test_checks` | `library/src/repo_checks.rs` `the_version_script_touches_every_place_this_test_checks` |
| `test_the_python_package_reads_the_version_from_one_place` | no se pasa: es el pyproject.toml del paquete de Python |
| `test_the_api_does_not_repeat_the_version` | `library/src/repo_checks.rs` `the_api_does_not_repeat_the_version` (la del servidor sale de `env!("CARGO_PKG_VERSION")`) |
| `test_the_version_looks_like_a_version` | `library/src/repo_checks.rs` `the_version_looks_like_a_version` |
| `test_nothing_hardcodes_the_version_in_a_file_name` | `library/src/repo_checks.rs` `nothing_hardcodes_the_version_in_a_file_name` |
| `test_the_declared_associations_match_the_library` | `library/src/repo_checks.rs` `the_declared_associations_match_the_library` |
| `test_every_association_declares_the_type_the_api_serves` | `server/src/services/media.rs` `every_association_declares_the_type_the_api_serves` |
| `test_rust_knows_the_same_types` | `library/src/repo_checks.rs` `the_app_knows_the_same_types` |
| `test_the_desktop_entry_passes_the_file_to_the_app` | `library/src/repo_checks.rs` `the_desktop_entry_passes_the_file_to_the_app` |
| `test_external_tools_are_looked_up_next_to_the_executable` | `library/src/config/tests.rs` `external_tools_are_looked_up_next_to_the_executable` |
| `test_on_windows_the_exe_suffix_is_tried` | `library/src/config/tests.rs` `on_windows_the_exe_suffix_is_tried` (con `find_tool_with`, los sufijos de Windows también en Linux) |
| `test_reserved_windows_names_get_out_of_the_way` | `library/src/names/tests.rs` `reserved_windows_names_get_out_of_the_way` |
| `test_names_do_not_end_in_a_dot_or_a_space` | `library/src/names/tests.rs` `names_do_not_end_in_a_dot_or_a_space` |
| `test_being_inside_the_library_is_decided_by_components` | `library/src/index/folders.rs` `being_inside_the_library_is_decided_by_components` |
| `test_the_detector_recognizes_the_whole_narration_corpus` | `library/src/assistant/narration.rs` `the_detector_recognizes_the_whole_narration_corpus` (el corpus, copiado a `library/src/assistant/narration/corpus.json`) |
| `test_the_detector_does_not_fire_on_informative_text` | `library/src/assistant/narration.rs` `the_detector_does_not_fire_on_informative_text` |
| `test_the_tolerated_cases_are_still_the_intended_ones` | `library/src/assistant/narration.rs` `the_tolerated_cases_are_still_the_intended_ones` |
| `test_the_video_kind_comes_from_unambiguous_words` | `library/src/names/tests.rs` `the_video_kind_comes_from_unambiguous_words` |
| `test_a_drum_cam_is_by_the_drummer_not_the_original_artist` | `library/src/names/tests.rs` `a_drum_cam_is_by_the_drummer_not_the_original_artist` |
| `test_an_artist_you_already_have_wins_in_any_order` | `library/src/names/tests.rs` `an_artist_you_already_have_wins_in_any_order` |
| `test_the_channel_inside_the_title_is_the_artist` | `library/src/names/tests.rs` `the_channel_inside_the_title_is_the_artist` |
| `test_artist_dash_title_is_the_usual_order` | `library/src/names/tests.rs` `artist_dash_title_is_the_usual_order` |
| `test_no_dash_channel_is_artist_and_youtube_music_wins` | `library/src/names/tests.rs` `no_dash_channel_is_artist_and_youtube_music_wins` |
| `test_process_with_a_known_name_skips_the_cascade` | `library/src/ingest/tests.rs` `a_process_with_a_known_name_skips_the_cascade` (con lo de fuera de mentira, que apunta si se pidió la huella o la IA) |
| `test_the_windows_installer_upgrades_instead_of_duplicating` | `library/src/repo_checks.rs` `the_windows_installer_upgrades_instead_of_duplicating` |

## tests/test_config_rust.py

En Python se comparaba la configuración de Rust con la de danplay/config.py
y con python-dotenv. Sin Python, se compara con lo que daban ellas.

| Prueba de Python | En Rust |
|---|---|
| `test_rust_config_matches_the_python_config` | `library/src/config/tests.rs` `config_reads_the_environment_and_the_settings_file` (en otro proceso con ese entorno, como en Python; los valores, los que daba config.py) |
| `test_settings_file_is_read_like_python_dotenv` | `library/src/config/tests.rs` `settings_file_is_read_like_python_dotenv` (400 textos hechos con las mismas líneas, y lo que leía python-dotenv 1.2.4, generados una vez en `library/src/config/dotenv_cases.json`) |
| `test_saved_values_are_written_like_python_and_read_back` | `library/src/config/tests.rs` `saved_values_are_written_like_python_and_read_back` (los ejemplos fijos con lo que escribía config.py, y 2000 textos de un generador fijo en vez de Hypothesis: se escriben como en config.py y se releen iguales) |

## tests/test_resolve.py

El emparejador. Lo que decide cada canción (`Index::matching`) va en
`library/src/assistant/matching/tests/library_cases.rs` (abreviado
`L` aquí abajo), contra una base temporal con las mismas filas a mano; el
lote, el índice por revisión y su cerrojo, en
`server/src/services/chat/resolve/tests.rs` (abreviado `S`). Lo que en
Python era Hypothesis o `random`, con un generador fijo.

| Prueba de Python | En Rust |
|---|---|
| `test_exact_title_is_found` | `L` `exact_title_is_found` y `S` `exact_title_query` (la `query`; `index` no existe en Rust: es la posición en la lista) |
| `test_three_versions_is_ambiguous_with_one_chosen` | `L` `three_versions_is_ambiguous_with_one_chosen` |
| `test_the_requested_version_is_no_longer_ambiguous` | `L` `the_requested_version_is_no_longer_ambiguous` |
| `test_a_noisy_query_finds_the_same_song` | `L` `a_noisy_query_finds_the_same_song` |
| `test_whatsapp_decorations_do_not_get_in_the_way` | `L` `whatsapp_decorations_do_not_get_in_the_way` |
| `test_a_wrong_artist_or_a_mention_does_not_hide_the_song` | `L` `a_wrong_artist_or_a_mention_does_not_hide_the_song` |
| `test_the_right_artist_only_adds` | `L` `the_right_artist_only_adds` |
| `test_the_artist_attached_or_first` | `L` `the_artist_attached_or_first` |
| `test_dirty_tags_are_searched_by_file_and_by_tag` | `L` `dirty_tags_are_searched_by_file_and_by_tag` |
| `test_crossed_tags_are_found_with_the_artist` | `L` `crossed_tags_are_found_with_the_artist` |
| `test_in_house_copies_are_the_same_song` | `L` `in_house_copies_are_the_same_song` |
| `test_titles_made_of_stop_words` | `L` `titles_made_of_stop_words` |
| `test_enye_is_not_n_in_the_normal_form` | `L` `enye_is_not_n_in_the_normal_form` |
| `test_the_second_pass_forgives_enye_at_a_discount` | `L` `the_second_pass_forgives_enye_at_a_discount` |
| `test_enye_and_n_do_not_count_as_a_typo` | `L` `enye_and_n_do_not_count_as_a_typo` (parte ya estaba en `library/src/assistant/matching/tests.rs` `typos_are_near`) |
| `test_apostrophes_in_titles_and_artists` | `L` `apostrophes_in_titles_and_artists` |
| `test_the_artist_apostrophe_with_a_gap_too` | `L` `the_artist_apostrophe_with_a_gap_too` |
| `test_a_typo_is_probable_and_finds_the_song` | `L` `a_typo_is_probable_and_finds_the_song` |
| `test_a_typo_with_the_right_artist_is_found` | `L` `a_typo_with_the_right_artist_is_found` |
| `test_a_typo_in_a_short_word_only_shows_when_joined` | `L` `a_typo_in_a_short_word_only_shows_when_joined` |
| `test_the_start_of_a_title_is_probable` | `L` `the_start_of_a_title_is_probable` |
| `test_the_same_words_in_another_order_is_probable` | `L` `the_same_words_in_another_order_is_probable` |
| `test_words_joined_or_split_differently` | `L` `words_joined_or_split_differently` |
| `test_a_subtitle_and_part_of_a_medley` | `L` `a_subtitle_and_part_of_a_medley` |
| `test_what_is_missing_is_not_matched` | `L` `what_is_missing_is_not_matched` |
| `test_what_is_missing_with_its_artist_is_missing_too` | `L` `what_is_missing_with_its_artist_is_missing_too` |
| `test_an_extra_word_is_never_found` | `L` `an_extra_word_is_never_found` |
| `test_a_changed_word_is_never_found` | `L` `a_changed_word_is_never_found` |
| `test_an_already_downloaded_link_beats_the_title` | `L` `an_already_downloaded_link_beats_the_title` |
| `test_the_link_is_the_same_video_however_it_is_written` | `L` `the_link_is_the_same_video_however_it_is_written` |
| `test_what_is_not_a_youtube_video_is_not_a_link` | `L` `what_is_not_a_youtube_video_is_not_a_link` |
| `test_a_reused_id_does_not_count` | `L` `a_reused_id_does_not_count` |
| `test_history_is_walked_from_newest_to_oldest` | `L` `history_is_walked_from_newest_to_oldest` |
| `test_the_same_file_name_counts_even_if_the_folder_moved` | `L` `the_same_file_name_counts_even_if_the_folder_moved` |
| `test_already_there_is_checked_by_the_name_key` | `L` `already_there_is_checked_by_the_name_key` |
| `test_other_version_and_strict` | `L` `other_version_and_strict` |
| `test_the_video_title` | `L` `the_video_title` |
| `test_a_link_pasted_in_the_title_is_not_the_title` | `L` `a_link_pasted_in_the_title_is_not_the_title` |
| `test_resolve_items_in_order_with_each_status` | `S` `resolve_items_in_order_with_each_status` |
| `test_overrides_and_explicit_id` | `S` `overrides_and_explicit_id` |
| `test_the_remembered_choice_wins_ties` | `L` `the_remembered_choice_wins_ties` y `S` `the_remembered_choice_wins_ties_in_the_batch` (los `prefer_ids` que no son números, "no" y None, se quedaban en Python: en Rust ya llegan como números) |
| `test_title_key_is_stable_and_tells_apart` | `L` `title_key_is_stable_and_tells_apart` (sin `title_key(None)`: en Rust el título siempre es texto) |
| `test_tiebreaks_choice_favorite_stars_and_short_title` | `L` `tiebreaks_choice_favorite_stars_and_short_title` |
| `test_an_item_that_crashes_does_not_sink_the_batch` | `S` `an_item_that_crashes_does_not_sink_the_batch` (en Python se hacía reventar `_match`; aquí revienta de verdad la consulta de ese item: su id no está en memoria y la tabla ya no se puede leer) |
| `test_the_time_budget_and_the_item_cap` | `S` `the_time_budget_and_the_item_cap` (con `resolve_items_within`, el presupuesto y el tope como parámetros; y el tope de verdad, con 201 items) |
| `test_a_failing_db_does_not_sink_the_batch` | `S` `a_failing_db_does_not_sink_the_batch` (una base que no se puede abrir, en vez de cambiar `INDEX.snapshot`) |
| `test_a_setlist_of_60` | `S` `a_setlist_of_60` |
| `test_rank_is_the_measure_search_adapter` | `L` `rank_is_the_measure_search_adapter` (el adaptador del medidor no existe en Rust; la prueba lo arma con `matching(limit=10)`, como hacía `resolve.rank`) |
| `test_limit_and_the_copies_returned` | `L` `limit_and_the_copies_returned` |
| `test_hostile_inputs_within_a_time_limit` | `L` `hostile_inputs_within_a_time_limit` (los dos modos: en memoria y con el prefiltro FTS5) |
| `test_a_whole_hostile_batch_does_not_fail` | `L` `a_whole_hostile_batch_does_not_fail` (ninguna consulta da error, que es lo que en el lote sería `failed`) |
| `test_fts_syntax_does_not_change_the_search` | `L` `fts_syntax_does_not_change_the_search` |
| `test_a_title_of_only_stop_words_and_one_letter` | `L` `a_title_of_only_stop_words_and_one_letter` |
| `test_any_text_gives_a_valid_answer` | `L` `any_text_gives_a_valid_answer` (300 textos de un generador fijo en vez de 120 de Hypothesis) |
| `test_the_fts_prefilter_matches_the_in_memory_index` | `L` `the_fts_prefilter_matches_the_in_memory_index` |
| `test_the_prefilter_finds_a_typo_with_the_tolerant_retry` | `L` `the_prefilter_finds_a_typo_with_the_tolerant_retry` |
| `test_the_index_is_rebuilt_with_the_library_revision` | `S` `the_index_is_rebuilt_with_the_library_revision` |
| `test_another_db_another_index` | `S` `another_db_another_index` |
| `test_the_index_is_built_once_under_a_lock` | `S` `the_index_is_built_once_under_a_lock` (los ocho hilos reciben el mismo índice, en vez de contar las llamadas a `load`) |
| `test_the_song_data_comes_from_the_db` | `L` `the_song_data_comes_from_the_db` |
| `test_50000_songs_within_a_time_limit` | `L` `songs_50000_within_a_time_limit` (los topes de Python; sin optimizar, diez veces más) |

## tests/test_ai.py

En Python el «proveedor» era un cliente del SDK de OpenAI de mentira; en
Rust es un servidor HTTP en el propio equipo que contesta según lo que le
piden (`serve_fn`, y `nobody_home` para uno apagado). Abreviaturas:
`P` = `library/src/ai/providers/tests.rs`, `K` = `library/src/ai/catalog.rs`,
`C` = `library/src/ai/call/tests.rs`, `E` = `library/src/enrich/tests.rs`,
`T` = `library/src/assistant/tools/tests.rs`,
`A` = `server/src/services/chat/tests/ai_cases.rs` (un turno entero por
`/api/chat` y las herramientas con su validación).

| Prueba de Python | En Rust |
|---|---|
| `test_catalog_has_groups_and_well_formed_urls` | `P` `catalog_has_groups_and_well_formed_urls` |
| `test_local_providers_need_no_key_and_point_here` | `P` `local_providers_need_no_key_and_point_here` |
| `test_with_nothing_configured_ai_is_unavailable` | `P` `with_nothing_configured_ai_is_unavailable` |
| `test_saving_profile_activates_it_and_key_comes_back_masked` | `P` `saving_profile_activates_it_and_key_comes_back_masked` |
| `test_saving_without_key_keeps_the_saved_one` | `P` `saving_without_key_keeps_the_saved_one` |
| `test_url_placeholders_are_filled_from_the_fields` | `P` `url_placeholders_are_filled_from_the_fields` |
| `test_several_own_servers_coexist_and_can_be_switched` | `P` `several_own_servers_coexist_and_can_be_switched` |
| `test_local_without_key_is_activated_as_is` | `P` `local_without_key_is_activated_as_is` |
| `test_env_vars_override_the_file` | `P` `env_vars_override_the_file` (en otro proceso con esas variables) |
| `test_old_deepinfra_key_is_migrated_automatically` | `P` `old_deepinfra_key_is_migrated_automatically` (ídem) |
| `test_catalog_trim_keeps_only_chat_models` | `K` `catalog_trim_keeps_only_chat_models` |
| `test_bundled_snapshot_exists_and_knows_the_big_ones` | `K` `bundled_snapshot_exists_and_knows_the_big_ones` (la foto de danplay/data: ver la nota del final) |
| `test_recommendation_skips_deprecated_experimental_toolless` | `K` `recommendation_skips_deprecated_experimental_toolless` (con la fecha de hoy fija) |
| `test_offline_catalog_breaks_nothing` | `K` `offline_catalog_breaks_nothing` (lo que hace el servidor cuando models.dev no contesta; la descarga en sí va siempre a models.dev y sin red no se prueba) |
| `test_json_rejected_is_retried_without_it_and_remembered` | `C` `json_rejected_is_retried_without_it_and_remembered` |
| `test_required_tool_choice_falls_back_to_auto_if_unsupported` | `C` `required_tool_choice_falls_back_to_auto_if_unsupported` |
| `test_model_without_tools_is_reported_clearly` | ya estaba en `C` `a_model_without_tools`; lo que dice el asistente, `A` `model_without_tools_is_reported_clearly` |
| `test_openai_reasoners_get_no_temperature` | `C` `openai_reasoners_get_no_temperature` (con la foto del catálogo) |
| `test_profile_extra_params_go_with_every_request` | `C` `profile_extra_params_go_with_every_request` (`extra_body` va mezclado en el cuerpo, como el SDK) |
| `test_check_tells_bad_key_from_no_connection` | `C` `check_tells_bad_key_from_no_connection` |
| `test_check_verifies_the_chat_model_uses_tools` | `C` `check_verifies_the_chat_model_uses_tools` |
| `test_model_list_is_crossed_with_the_catalog` | `C` `model_list_is_crossed_with_the_catalog` |
| `test_only_models_the_key_can_use_are_shown` | `C` `only_models_the_key_can_use_are_shown` |
| `test_local_server_models_are_not_checked_one_by_one` | `C` `local_server_models_are_not_checked_one_by_one` |
| `test_draft_without_key_uses_the_saved_one` | `P` `draft_without_key_uses_the_saved_one` |
| `test_free_providers_come_first_and_need_no_key` | `P` `free_providers_come_first_and_need_no_key` |
| `test_try_free_activates_the_first_that_answers` | `C` `try_free_activates_the_first_that_answers` (con `try_free_at`: cada gratuito, en la dirección del servidor de mentira) |
| `test_try_free_with_none_alive_says_so_activates_none` | `C` `try_free_with_none_alive_says_so_activates_none` |
| `test_without_key_list_only_shows_anonymous_models` | `C` `without_key_list_only_shows_anonymous_models` |
| `test_toon_follows_the_spec` | `library/src/toon.rs` `toon_follows_the_spec` |
| `test_tools_reach_the_model_in_toon_and_weigh_less` | `A` `tools_reach_the_model_in_toon_and_weigh_less` (con 20 canciones de verdad en el índice en vez de un `run_tool` de mentira) |
| `test_empty_ai_details_are_not_reused` | `E` `empty_ai_details_are_not_reused` (parte ya estaba en `the_saved_details_keep_the_sheet`) |
| `test_chunked_reply_is_joined_and_shown_as_it_arrives` | `C` `chunked_reply_is_joined_and_shown_as_it_arrives` |
| `test_chunked_tool_calls_are_rebuilt_and_preamble_dropped` | `C` `chunked_tool_calls_are_rebuilt_and_preamble_dropped` (parecida ya estaba en `streamed_with_preamble_and_tool`) |
| `test_unclosed_reasoning_is_not_shown_half_done` | `C` `unclosed_reasoning_is_not_shown_half_done` |
| `test_cancel_cuts_the_reply_and_closes_the_stream` | `C` `cancel_cuts_the_reply_and_closes_the_stream` (el servidor deja el flujo abierto: la llamada vuelve sin esperarlo y el servidor ve que se cortó) |
| `test_fallback_answers_when_the_active_one_is_down` | `C` `fallback_answers_when_the_active_one_is_down` (el cliente de Rust reintenta el 503 como el SDK; el falso de Python no lo hacía) |
| `test_message_error_does_not_trigger_the_fallback` | `C` `message_error_does_not_trigger_the_fallback` |
| `test_usage_is_logged_per_turn_and_in_the_db` | `C` `usage_is_logged_per_turn_and_in_the_db` |
| `test_identify_asks_for_schema_if_the_model_supports_it` | `C` `identify_asks_for_schema_if_the_model_supports_it` |
| `test_screen_state_tells_what_is_seen_selected_playing` | `library/src/assistant/context.rs` `screen_state_tells_what_is_seen_selected_playing` |
| `test_context_reaches_the_model_as_screen_state` | `A` `context_reaches_the_model_as_screen_state` |
| `test_neighbor_keys` | `library/src/theory.rs` `neighbor_keys` |
| `test_chats_are_saved_and_searchable` | `library/src/chats.rs` `chats_are_saved_and_searchable` (por la API ya estaba en `server/src/api_tests/ai.rs` `chats_are_kept_and_searchable`) |
| `test_tool_call_written_as_text_still_runs` | `library/src/assistant/narration.rs` `a_tool_call_written_as_text_is_understood` y `A` `tool_call_written_as_text_still_runs` |
| `test_stream_broken_halfway_is_retried_whole` | `C` `stream_broken_halfway_is_retried_whole` (la primera parte ya estaba en `a_broken_stream_is_retried_whole`) |
| `test_judge_is_only_called_when_an_action_was_asked` | `library/src/assistant/narration.rs` `an_action_is_asked_only_when_it_is_asked` y `A` `judge_is_only_called_when_an_action_was_asked` |
| `test_optional_tools_only_sent_when_the_chat_needs_them` | `A` `optional_tools_only_sent_when_the_chat_needs_them` (parte ya estaba en `T` `only_what_the_conversation_needs_is_declared`) |
| `test_budget_only_warns` | `A` `budget_only_warns` |
| `test_chat_is_exported_as_text` | `A` `chat_is_exported_as_text` (parte ya estaba en `server/src/routes/assistant.rs` `saved_conversations`) |
| `test_only_a_json_object_counts_as_a_reply` | `C` `only_a_json_object_counts_as_a_reply` |
| `test_details_survive_a_reply_that_is_not_an_object` | `E` `details_survive_a_reply_that_is_not_an_object` |
| `test_non_numeric_confidence_breaks_nothing` | `E` `non_numeric_confidence_breaks_nothing` |
| `test_null_arguments_do_not_crash_the_chat` | `A` `null_arguments_do_not_crash_the_chat` |
| `test_tools_have_limits` | `T` `tools_have_limits` |
| `test_assistant_edit_only_touches_declared_fields` | `T` `assistant_edit_only_touches_declared_fields` |
| `test_timed_lyrics_do_not_reach_the_uslt` | `E` `timed_lyrics_do_not_reach_the_uslt` (parte ya estaba en `enrich_saves_lyrics_cover_and_details`) |
| `test_every_declared_tool_has_a_handler` | `A` `every_declared_tool_has_a_handler` |
| `test_invalid_arguments_are_reported_in_spanish` | `A` `invalid_arguments_are_reported_in_spanish` (parte ya estaba en `server/src/services/chat/tests.rs` `arguments_like_pydantic`) |
| `test_numbers_as_text_and_year_as_number_are_accepted` | `A` `numbers_as_text_and_year_as_number_are_accepted` |
| `test_old_names_still_work` | `A` `old_names_still_work` (con un mp3 de verdad: sin archivo, poner estrellas falla), `server/src/services/chat/tools.rs` `old_names_become_the_new_ones` y `T` `lyrics_without_id_go_by_artist_and_title` (la letra por nombre de verdad va a LRCLIB, que no se prueba sin red) |

## tests/test_ai_status.py

Los errores, servidos por HTTP a este mismo cliente con los cuerpos reales
de cada proveedor (en Python los hacía el constructor del SDK de openai); la
hora, el reloj a mano de las pruebas. `S` =
`library/src/ai/call/tests/status.rs`, `A` =
`server/src/services/chat/tests/ai_cases.rs`.

| Prueba de Python | En Rust |
|---|---|
| `test_real_errors_are_classified_by_text_before_code` | `S` `real_errors_are_classified_by_text_before_code` (la tabla `ERROR_CASES` entera) |
| `test_timeout_and_lost_connection_are_down_without_code` | `S` `timeout_and_lost_connection_are_down_without_code` (el tiempo agotado y la conexión perdida, de verdad) |
| `test_code_is_read_from_the_text_if_the_exception_lacks_it` | `S` `code_is_read_from_the_text_if_the_exception_lacks_it` |
| `test_how_long_it_stays_down_depends_on_why_it_failed` | `S` `how_long_it_stays_down_depends_on_why_it_failed` (los cinco casos) |
| `test_the_mark_is_set_on_failure_not_before_the_call` | `S` `the_mark_is_set_on_failure_not_before_the_call` |
| `test_no_credit_next_turn_goes_to_fallback_for_20_minutes` | `S` `no_credit_next_turn_goes_to_fallback_for_20_minutes` |
| `test_failure_is_forgotten_when_the_primary_answers_again` | `S` `failure_is_forgotten_when_the_primary_answers_again` |
| `test_a_message_error_marks_nobody` | `S` `a_message_error_marks_nobody` |
| `test_without_fallback_primary_is_still_called_problem_shows` | `S` `without_fallback_primary_is_still_called_problem_shows` |
| `test_primary_model_without_tools_falls_back_with_that_reason` | `S` `primary_model_without_tools_falls_back_with_that_reason` |
| `test_status_without_any_profile_is_empty` | `S` `status_without_any_profile_is_empty` |
| `test_status_has_the_contract_shape` | `S` `status_has_the_contract_shape` |
| `test_status_is_visible_from_another_thread` | `S` `status_is_visible_from_another_thread` |
| `test_status_survives_concurrent_readers_and_writers` | `S` `status_survives_concurrent_readers_and_writers` |
| `test_judge_and_identify_calls_do_not_change_who_answers` | `S` `judge_and_identify_calls_do_not_change_who_answers` |
| `test_the_turn_request_counter_misses_none` | `S` `the_turn_request_counter_misses_none` |
| `test_primary_failure_elsewhere_is_noted_but_answer_unchanged` | `S` `primary_failure_elsewhere_is_noted_but_answer_unchanged` |
| `test_recheck_clears_the_mark_and_next_message_tries_primary` | `S` `recheck_clears_the_mark_and_next_message_tries_primary` |
| `test_reset_client_forgets_the_failure_and_who_answered` | `S` `reset_client_forgets_the_failure_and_who_answered` |
| `test_active_check_clears_the_mark_but_a_draft_check_does_not` | `S` `active_check_clears_the_mark_but_a_draft_check_does_not` (el borrador con la URL del servidor de mentira: sin ella iría a Anthropic de verdad) |
| `test_recheck_route_clears_the_mark_and_returns_the_status` | `A` `recheck_route_clears_the_mark_and_returns_the_status` |
| `test_real_sdk_primary_without_credit_gets_a_call_per_20_min` | `S` `primary_without_credit_gets_a_call_per_20_min` (sin SDK: el cliente de Rust, por un socket de verdad) |
| `test_chat_tools_returns_the_new_fields_and_the_usual_ones` | `A` `chat_tools_returns_the_new_fields_and_the_usual_ones` (con la hora de verdad: del «at» se mira que sea un número) |
| `test_the_loop_reports_who_answered_and_why` | `A` `the_loop_reports_who_answered_and_why` |
| `test_no_provider_secret_in_status_route_error_or_logs` | `S` `no_provider_secret_in_status_errors_or_logs` (el estado, el error y el registro) y `A` `no_provider_secret_in_the_tools_route_or_the_chat_error` (la franja y el error del chat), con las once fugas |
| `test_describe_error_tells_rate_limit_billing_and_403_apart` | `S` `describe_error_tells_rate_limit_billing_and_403_apart` |
| `test_sanitize_removes_secrets_and_keeps_the_rest` | `S` `sanitize_removes_secrets_and_keeps_the_rest` |
| `test_sanitize_gives_one_line_of_160_max_without_invisibles` | `S` `sanitize_gives_one_line_of_160_max_without_invisibles` |
| `test_sanitize_never_raises_whatever_it_gets` | `S` `sanitize_never_fails_whatever_it_gets` (lo que tiene sentido con un texto; `None`, números, bytes y un objeto cuyo `str()` revienta eran cosas de Python) |
| `test_the_sdk_preloads_in_the_background_only_with_ai` | no se pasa: precargaba el SDK de openai de Python; el cliente de Rust no tiene nada que cargar |

En el servidor, quién contestó y la marca de «caído» son de todo el proceso
y guardar un perfil (en cualquier prueba) las borra: las pruebas de `A` van
de una en una y lo que cuenta llamadas lo intenta hasta tres veces.

## tests/test_playlists_place.py

El cerrojo, el aviso (`touch`) y el sellado de los archivos son del servidor
(`services::playlists`, `services::chat::lists::place`): todo va en
`server/src/services/playlists/tests.rs` (`P`), contra la base de verdad. Lo
que Python apuntaba con un `monkeypatch` de `_stamp_playlists_into_files`
aquí se ve en los archivos: canciones con su mp3 de verdad, y las que se
sellan cambian de fecha (sin ffmpeg, esas pruebas no comprueban nada).

| Prueba de Python | En Rust |
|---|---|
| `test_place_inserts_what_is_missing_and_orders_everything` | `P` `place_inserts_what_is_missing_and_orders_everything` |
| `test_place_puts_each_new_one_right_after_its_previous` | `P` `place_puts_each_new_one_right_after_its_previous` |
| `test_place_two_new_in_a_row_and_one_first_land_as_asked` | `P` `place_two_new_in_a_row_and_one_first_land_as_asked` |
| `test_place_leaves_the_unmentioned_behind_in_order` | `P` `place_leaves_the_unmentioned_behind_in_order` |
| `test_place_on_an_empty_playlist_and_with_nothing_to_add` | `P` `place_on_an_empty_playlist_and_with_nothing_to_add` (el id suelto, por `lists::ints`) |
| `test_place_keeps_songs_from_outside_the_library` | `P` `place_keeps_songs_from_outside_the_library` |
| `test_place_ignores_what_is_not_a_song` | `P` `place_ignores_what_is_not_a_song` (lo de fuera leído con `lists::ints`, como lo lee el asistente) |
| `test_place_does_not_repeat_duplicates` | `P` `place_does_not_repeat_duplicates` |
| `test_place_stamps_tags_once_and_only_for_new_ones` | `P` `place_stamps_tags_once_and_only_for_new_ones` (cuáles se sellan; «una vez» no se ve desde fuera) |
| `test_place_is_idempotent` | `P` `place_is_idempotent` |
| `test_place_signals_that_the_library_changed` | `P` `place_signals_that_the_library_changed` |
| `test_place_on_a_missing_playlist_writes_nothing` | `P` `place_on_a_missing_playlist_writes_nothing` (sin «no» ni None: en Rust el id ya es un número) |
| `test_place_keeps_a_song_with_a_gone_file_behind_and_hidden` | `P` `place_keeps_a_song_with_a_gone_file_behind_and_hidden` |
| `test_place_repairs_repeated_or_gapped_positions` | `P` `place_repairs_repeated_or_gapped_positions` |
| `test_place_with_more_ids_than_sqlite_variables` | `P` `place_with_more_ids_than_sqlite_variables` |
| `test_place_writes_playlist_name_into_the_file_only_if_needed` | `P` `place_writes_playlist_name_into_the_file_only_if_needed` |
| `test_add_appends_and_counts_only_those_that_go_in` | `P` `add_appends_and_counts_only_those_that_go_in` |
| `test_add_stamps_the_tags_of_what_is_requested` | `P` `add_stamps_the_tags_of_what_is_requested` |
| `test_reorder_names_only_some_and_the_rest_stay_behind` | `P` `reorder_names_only_some_and_the_rest_stay_behind` |
| `test_reorder_does_not_add_what_the_playlist_lacks` | `P` `reorder_does_not_add_what_the_playlist_lacks` |
| `test_remove_song_and_remove_stamp_the_files` | `P` `remove_song_and_remove_stamp_the_files` |
| `test_renaming_stamps_the_songs_once` | `P` `renaming_stamps_the_songs_once` |
| `test_set_songs_leaves_the_playlist_exactly_so` | `P` `set_songs_leaves_the_playlist_exactly_so` |
| `test_set_songs_with_outside_songs` | `P` `set_songs_with_outside_songs` |
| `test_set_songs_stamps_the_tags_only_once` | `P` `set_songs_stamps_the_tags_only_once` |
| `test_set_songs_leaves_rows_of_songs_whose_file_is_gone` | `P` `set_songs_leaves_rows_of_songs_whose_file_is_gone` |
| `test_set_songs_matches_the_model_on_random_playlists` | `P` `set_songs_matches_the_model_on_random_playlists` (un generador fijo; la basura, ids que no existen) |
| `test_concurrent_place_add_reorder_lose_or_repeat_nothing` | `P` `concurrent_place_add_reorder_lose_or_repeat_nothing` |
| `test_set_songs_amid_everything_else_keeps_the_playlist_sound` | `P` `set_songs_amid_everything_else_keeps_the_playlist_sound` |
| `test_two_concurrent_places_do_not_clash_on_positions` | `P` `two_concurrent_places_do_not_clash_on_positions` |
| `test_read_and_set_under_the_lock_loses_no_one_elses_songs` | `P` `read_and_set_under_the_lock_loses_no_one_elses_songs` |
| `test_creating_the_same_playlist_at_once_gives_just_one` | `P` `creating_the_same_playlist_at_once_gives_just_one` |
| `test_whoever_holds_the_lock_can_call_the_other_functions` | `P` `whoever_holds_the_lock_can_call_the_other_functions` |
| `test_each_write_waits_for_the_lock` | `P` `each_write_waits_for_the_lock` (las ocho escrituras; `rename_folder` no se pasa: en Rust no lo llama nadie, y en Python tampoco fuera de esta prueba) |

## tests/test_safe.py

Todo en `library/src/safe.rs` (`F`). Las de Hypothesis, con un generador
fijo de texto (letras de todo Unicode y lo que suele romper).

**Diferencia encontrada y arreglada**: `datum` dejaba pasar caracteres de
formato (Cf) que no estaban en su lista, como U+0600 (marca de número árabe)
o U+070F (marca de abreviatura siria). La prueba de propiedades de Python los
pedía fuera (`assert_clean`), pero Hypothesis no llegó a generarlos; el
generador de Rust sí. Ahora `datum` (y `strip_invisible`) quitan cualquier
carácter de formato.

| Prueba de Python | En Rust |
|---|---|
| `test_a_normal_text_does_not_change` | `F` `a_normal_text_does_not_change` (sin `datum(None)` ni `datum(12)`: en Rust el dato ya es texto; lo que falta va como None en `data_block`) |
| `test_line_breaks_become_one_space` | `F` `line_breaks_become_one_space` |
| `test_controls_are_dropped_without_leaving_a_gap` | `F` `controls_are_dropped_without_leaving_a_gap` |
| `test_direction_marks_are_dropped` | `F` `direction_marks_are_dropped` |
| `test_zero_width_is_dropped` | `F` `zero_width_is_dropped` |
| `test_invisible_tag_characters_are_dropped` | `F` `invisible_tag_characters_are_dropped` |
| `test_odd_spaces_become_one_space` | `F` `odd_spaces_become_one_space` |
| `test_no_curly_quote_or_guillemet_remains` | `F` `no_curly_quote_or_guillemet_remains` |
| `test_a_value_cannot_close_its_own_quotes` | `F` `a_value_cannot_close_its_own_quotes` |
| `test_the_apostrophe_is_kept_as_an_apostrophe` | `F` `the_apostrophe_is_kept_as_an_apostrophe` |
| `test_the_tilde_composes_after_dropping_zero_width` | `F` `the_tilde_composes_after_dropping_zero_width` (parte ya estaba en `datum_without_disguise`) |
| `test_it_truncates_with_an_ellipsis` | `F` `it_truncates_with_an_ellipsis` |
| `test_the_default_is_80` | `F` `the_default_is_80` (en Rust no hay valor por defecto: quien llama pasa el 80) |
| `test_it_is_idempotent_in_the_hard_cases` | `F` `it_is_idempotent_in_the_hard_cases` |
| `test_any_output_is_a_clean_datum` | `F` `any_output_is_a_clean_datum_and_idempotent` |
| `test_datum_is_idempotent` | `F` `any_output_is_a_clean_datum_and_idempotent` |
| `test_any_text_never_crashes` | `F` `any_text_never_crashes` |
| `test_printable_ascii_is_only_truncated` | `F` `printable_ascii_is_only_truncated` |
| `test_a_huge_text_costs_no_more_than_a_short_one` | `F` `a_huge_text_costs_no_more_than_a_short_one` |
| `test_a_huge_text_of_only_invisibles_does_not_hang` | `F` `a_huge_text_of_only_invisibles_does_not_hang` |
| `test_the_block_has_the_header_and_values_in_guillemets` | `F` `the_block_has_the_header_and_values_in_guillemets` |
| `test_the_empty_block_says_so` | `F` `the_empty_block_says_so` |
| `test_the_block_passes_each_value_through_datum` | `F` `the_block_passes_each_value_through_datum` |
| `test_the_block_keeps_the_cap_and_says_how_many_are_left` | `F` `the_block_keeps_the_cap_and_says_how_many_are_left` (el tope de 60 lo pasa quien llama) |
| `test_the_block_accepts_a_generator` | `F` `the_block_accepts_a_generator` |
| `test_the_hostile_title_stays_enclosed` | `F` `the_hostile_title_stays_enclosed` |
| `test_the_block_is_one_line_with_each_value_enclosed` | `F` `the_block_is_one_line_with_each_value_enclosed` |

## tests/test_intake.py

El lector de listas pegadas: `library/src/assistant/intake/tests.rs` (`I`);
las muestras, copiadas a `library/src/assistant/intake/samples.json` (antes
`tests/fixtures/intake_samples.json`). `last_setlist` es del servidor:
`server/src/services/chat/tests/intake_cases.rs` (`S`). Las de Hypothesis,
con un generador fijo.

| Prueba de Python | En Rust |
|---|---|
| `test_the_data_contract` | `I` `the_data_contract` (los topes y lo que valen sin nada; el orden de los campos de las «dataclasses» era de Python) |
| `test_the_fixture_is_complete_and_clean` | `I` `the_fixture_is_complete_and_clean` |
| `test_sample_parses_as_expected` | `I` `sample_parses_as_expected` (las 94 muestras, con el mismo comparador) |
| `test_link_tracking_does_not_change_the_parse` | `I` `link_tracking_does_not_change_the_parse` |
| `test_the_same_setlist_with_any_spacing` | `I` `the_same_setlist_with_any_spacing` |
| `test_over_sixty_are_trimmed_and_the_excess_is_reported` | `I` `over_sixty_are_trimmed_and_the_excess_is_reported` |
| `test_oddities_that_crashed_or_faked_a_setlist` | `I` `oddities_that_crashed_or_faked_a_setlist` |
| `test_a_role_after_a_mention_is_the_note_not_a_surname` | `I` `a_role_after_a_mention_is_the_note_not_a_surname` |
| `test_what_is_not_text_is_not_a_setlist` | `I` `what_is_not_text_is_not_a_setlist` (None, números, bytes y listas eran de Python: quedan el texto vacío y el de solo espacios) |
| `test_read_intent` | `I` `read_intent_cases` (la tabla entera) |
| `test_read_intent_in_uppercase_and_without_text` | `I` `read_intent_in_uppercase_and_without_text` |
| `test_clean_youtube_url` | `I` `clean_youtube_url_cases` |
| `test_what_is_not_youtube_gives_empty` | `I` `what_is_not_youtube_gives_empty` |
| `test_what_is_not_text_gives_empty` | no se pasa: None, números, listas y bytes; en Rust la dirección siempre es texto |
| `test_pasted_setlist_keeps_yt_playlist_and_drops_the_mix` | `I` `pasted_setlist_keeps_yt_playlist_and_drops_the_mix` |
| `test_a_hostile_link_in_the_setlist_is_not_a_song` | `I` `a_hostile_link_in_the_setlist_is_not_a_song` |
| `test_last_setlist_takes_the_users_last_visible_setlist` | `S` `last_setlist_takes_the_users_last_visible_setlist` |
| `test_last_setlist_accepts_role_user_and_tolerates_junk` | `S` `last_setlist_accepts_role_user_and_tolerates_junk` |
| `test_last_setlist_without_a_setlist_gives_none` | `S` `last_setlist_without_a_setlist_gives_none` (sin lo que no es una lista de mensajes) |
| `test_last_setlist_only_looks_at_the_window` | `S` `last_setlist_only_looks_at_the_window` |
| `test_hostile_input_in_under_a_second` | `I` `hostile_input_in_under_a_second` (las 25 entradas; sin optimizar, diez veces más de tope) |
| `test_over_200_kb_is_cut_and_does_not_hang` | `I` `over_200_kb_is_cut_and_does_not_hang` |
| `test_the_parser_never_fails_and_keeps_the_contract` | `I` `the_parser_never_fails_and_keeps_the_contract` |
| `test_any_text_does_not_break_either` | `I` `any_text_does_not_break_either` |
| `test_clean_youtube_url_is_canonical_and_idempotent` | `I` `clean_youtube_url_is_canonical_and_idempotent` |

## tests/test_chord_sheets.py

`C` = `library/src/chord_sheets/tests.rs` (las páginas, con un `get` de
mentira: lo que no conoce es «sin red»); `E` = `library/src/enrich/tests.rs`.

| Prueba de Python | En Rust |
|---|---|
| `test_ultimate_guitar_parsed_by_sections_and_marked_chords` | ya estaba en `C` `ultimate_guitar_by_sections_and_marked_chords` |
| `test_sections_written_as_plain_text_also_count` | ya estaba en `C` `sections_written_as_text` |
| `test_key_and_capo_stated_in_the_text_itself` | `C` `key_and_capo_stated_in_the_text_itself` |
| `test_lacuerda_is_parsed_by_its_markup` | ya estaba en `C` `lacuerda_by_its_markup` |
| `test_same_song_even_if_it_comes_from_youtube` | `C` `same_song_even_if_it_comes_from_youtube` (los cinco casos) |
| `test_another_song_is_not_accepted` | `C` `another_song_is_not_accepted` (los cuatro) |
| `test_search_title_drops_noise_and_the_artist` | `C` `search_title_drops_noise_and_the_artist` |
| `test_ultimate_guitar_prefers_the_tab_that_states_its_key` | `C` `ultimate_guitar_prefers_the_tab_that_states_its_key` (con los cinco resultados; la parecida que ya estaba, `ultimate_guitar_prefers_the_one_with_a_key`, llevaba dos) |
| `test_without_a_key_in_the_data_the_text_key_counts` | `C` `without_a_key_in_the_data_the_text_key_counts` |
| `test_nothing_if_ultimate_guitar_does_not_have_it` | `C` `nothing_if_ultimate_guitar_does_not_have_it` |
| `test_lacuerda_through_the_artist_page` | `C` `lacuerda_through_the_artist_page` |
| `test_find_merges_sources_and_reports_the_failed_ones` | ya estaba en `C` `find_combines_the_sources` |
| `test_the_key_that_can_be_asserted` | `C` `the_key_that_can_be_asserted` (sin `sheet_key(None)`) |
| `test_what_is_found_is_saved_and_not_searched_again` | `E` `what_is_found_is_saved_and_not_searched_again` |
| `test_not_found_is_saved_but_a_network_failure_is_not` | ya estaba en `E` `chords_are_saved_unless_the_network_failed` |
| `test_filled_key_comes_from_the_chord_sheet_not_the_ai` | `E` `filled_key_comes_from_the_chord_sheet_not_the_ai` (la primera parte ya estaba en `autofill_says_what_is_missing_and_why`) |
| `test_search_404_means_nothing_and_title_alone_is_tried` | `C` `search_404_means_nothing_and_title_alone_is_tried` (la de antes, `a_search_404_means_nothing_found`, no miraba que se probara el título solo) |
| `test_channels_search_with_the_artist_name` | `C` `channels_search_with_the_artist_name` |

## tests/test_media.py

`K` = `library/src/convert/tests.rs`; `E` = `library/src/enrich/tests.rs`
(con el `Fake` de lo de fuera); `SC` = `server/src/services/convert.rs`, `SE`
= `server/src/services/enrich.rs` y `ST` = `server/src/services/tools.rs`
(sus `mod tests`). Para que la app crea que falta un programa que sí está en
el PATH, las pruebas usan `App::hide_tool` (solo en las pruebas).

| Prueba de Python | En Rust |
|---|---|
| `test_a_flac_becomes_mp3_with_its_tags` | `K` `a_flac_becomes_mp3_with_its_tags` (lo del «(2)» ya estaba en `K` `flac_becomes_mp3_without_overwriting`) |
| `test_what_is_not_converted` | `SC` `what_is_not_converted` |
| `test_batch_convert_and_dry_run` | `SC` `batch_convert_and_dry_run` |
| `test_a_large_cover_is_shrunk` | ya estaba en `K` `a_big_cover_shrinks_and_a_fine_one_does_not`; lo de «si ya vale, tal cual», en `K` `a_cover_that_already_fits_is_kept_as_is` |
| `test_sizes_come_from_the_image_header` | `K` `sizes_come_from_the_image_header` (los seis formatos; el que ese ffmpeg no escriba se salta) |
| `test_sizes_from_unusual_headers` | ya estaba en `K` `image_size_from_odd_headers` (lo cortado y lo que no existe dan error en Rust; el (0, 0) lo ponía la puerta de Python) |
| `test_lrclib_lyrics_with_and_without_timestamps` | `E` `lrclib_lyrics_with_and_without_timestamps` (la lectura de la respuesta ya estaba en `E` `lrclib_gives_plain_and_timed_lyrics`) |
| `test_lyrics_by_fuzzy_search_and_by_ai` | ya estaba en `E` `lyrics_by_exact_match_then_by_search` y `the_ai_lyrics_only_when_it_knows_them`; lo de sin red y el «NO_LA_SE» tal cual, en `E` `no_lyrics_without_network_nor_from_an_ai_that_does_not_know_them` |
| `test_only_a_real_image_under_the_cap_is_accepted` | ya estaba en `SE` `images_like_enrich_get` y `json_pages_and_errors` (con un servidor local) y en `E` `knows_an_image_by_its_first_bytes` |
| `test_the_cover_is_searched_on_itunes_then_musicbrainz` | ya estaba en `E` `the_cover_from_itunes_then_musicbrainz`; que el id raro no llegue a la URL y el bueno dé la carátula, en `E` `the_cover_from_musicbrainz_skips_an_odd_id` |
| `test_enrich_saves_lyrics_cover_and_metadata` | `SE` `enrich_saves_lyrics_cover_and_metadata` (una canción de verdad, con la letra, la carátula y la ficha preparadas; se mira el índice y el archivo). Con lo de fuera de mentira ya estaba en `E` `enrich_saves_lyrics_cover_and_details` |
| `test_transposing_a_chord_sheet_only_touches_the_chords` | ya estaba en `E` `transposes_only_the_chord_lines`; se le añadió mirar las líneas («Dios» no es un Re) |
| `test_without_a_key_it_transposes_by_semitones` | `E` `without_a_key_it_transposes_by_semitones` (lo de -1 con tono ya estaba en `transposes_only_the_chord_lines`) |
| `test_web_search_returns_title_link_and_summary` | ya estaba en `library/src/web.rs` `title_link_and_summary` |
| `test_without_network_the_web_returns_nothing` | ya estaba en `library/src/web.rs` `title_link_and_summary` (con un `get` que falla) |
| `test_fingerprint_says_what_it_is_missing` | `ST` `fingerprint_says_what_it_is_missing` (sin `fingerprint.AVAILABLE`, que era una variable del módulo de Python) |
| `test_fingerprint_picks_the_best_match` | `ST` `fingerprint_picks_the_best_match` (con la consulta de mentira; la elección ya estaba en `library/src/fingerprint.rs` `picks_the_best_match`) |

## tests/test_properties.py

En Python, con Hypothesis. proptest no está en el espacio de trabajo, así que
van con un generador fijo (`Gen` de `library/src/testkit.rs`, un xorshift):
`CASES` (3000) casos por propiedad, caracteres de todo Unicode mezclados con
lo que más rompe (`NASTY`: controles, marcas sueltas, invisibles, comillas,
emojis, los caracteres prohibidos en un nombre, los nombres reservados de
Windows…). Las de teoría musical, que son pocas combinaciones, prueban
todas: cada nota de la tabla con cada semitono de -36 a 36, y cada par de
notas. `N` = `library/src/names/tests.rs` (`mod properties`), `T` =
`library/src/theory.rs` y `O` = `library/src/toon.rs` (sus `mod tests`).

| Prueba de Python | En Rust |
|---|---|
| `test_sanitize_always_gives_a_valid_name` | `N` `sanitize_always_gives_a_valid_name` |
| `test_the_final_name_is_a_valid_file_name` | `N` `the_final_name_is_a_valid_file_name` |
| `test_strip_accents_only_removes_marks` | `N` `strip_accents_only_removes_marks` |
| `test_strip_accents_leaves_ascii_alone` | `N` `strip_accents_leaves_ascii_alone` |
| `test_clean_leaves_no_junk_at_the_edges_nor_controls` | `N` `clean_leaves_no_junk_at_the_edges_nor_controls` |
| `test_the_match_key_is_stable` | `N` `the_match_key_is_stable` |
| `test_splitting_artist_and_title_never_crashes` | `N` `splitting_artist_and_title_never_crashes` |
| `test_transposing_a_note_moves_exactly_those_semitones` | `T` `transposing_a_note_moves_exactly_those_semitones` (todas) |
| `test_the_round_trip_distance_is_an_octave` | `T` `the_round_trip_distance_is_an_octave` (todos los pares) |
| `test_transposing_and_undoing_gives_the_same_pitches` | `T` `transposing_and_undoing_gives_the_same_pitches` |
| `test_transposing_to_the_same_key_changes_nothing` | `T` `transposing_to_the_same_key_changes_nothing` |
| `test_the_capo_is_always_between_1_and_7` | `T` `the_capo_is_always_between_1_and_7` (todas las notas) |
| `test_a_text_round_trips_through_toon` | `O` `a_text_round_trips_through_toon` (lo de entre comillas se lee con serde_json) |
| `test_toon_encodes_anything` | `O` `toon_encodes_anything` (enteros de cualquier tamaño, infinitos y -0.0 incluidos; sin NaN, como en Python) |
| `test_a_list_of_uniform_rows_is_a_table` | `O` `a_list_of_uniform_rows_is_a_table` |

## tests/test_compat.py

En Python, cada regla de `library::py` se comparaba en vivo con la de verdad
(a través de `danplay_core._compat`) sobre casos de Hypothesis. Sin Python,
lo que respondían `posixpath`, `ntpath`, `fnmatch`, `round`, `repr`,
`float`, `int` y `json` se guardó una vez (Python 3.13.5, Unicode 15.1) en
`library/src/py/tests/compat_cases.json`: casos de la misma forma que los de
Hypothesis, unos cientos o miles por regla, con sus `@example`, y los
decimales como sus bits. `C` = `library/src/py/tests/compat.rs`.

| Prueba de Python | En Rust |
|---|---|
| `test_posix_paths_like_posixpath` | `C` `posix_paths_like_posixpath` |
| `test_join_and_commonpath_like_posixpath` | `C` `join_and_commonpath_like_posixpath` |
| `test_windows_paths_like_ntpath` | `C` `windows_paths_like_ntpath` |
| `test_join_and_commonpath_like_ntpath` | `C` `join_and_commonpath_like_ntpath` |
| `test_real_filesystem_paths` | `C` `real_filesystem_paths` (en otro proceso, el de las pruebas otra vez, que trabaja desde la carpeta temporal y con su HOME: `real_paths_probe`; lo esperado es lo que daba `os.path` con esa carpeta) |
| `test_fnmatch_like_fnmatchcase` | `C` `fnmatch_like_fnmatchcase` |
| `test_round_like_python` | `C` `round_like_python` (mirando también el signo del cero, que el `==` de Python no miraba) |
| `test_float_repr_like_python` | `C` `float_repr_like_python` |
| `test_float_and_int_from_text_like_python` | `C` `float_and_int_from_text_like_python` |
| `test_repr_like_python` | `C` `repr_like_python` |
| `test_isidentifier_like_python` | `C` `isidentifier_like_python` |
| `test_isprintable_like_python` | `C` `isprintable_like_python` (todos los caracteres que ese Python conoce, no una muestra) |
| `test_json_dumps_like_python` | `C` `json_dumps_like_python` |

Los ejemplos fijos de antes siguen en `library/src/py/tests.rs`.

## tests/test_schema.py

`S` = `library/src/index/schema/tests.rs`. Lo que en Python se hacía con
`monkeypatch` de `db.SCHEMA` y `db.MIGRATIONS` va con `prepare_with`, que
recibe el esquema y las migraciones (`prepare` le pasa los de verdad).

| Prueba de Python | En Rust |
|---|---|
| `test_a_new_db_starts_at_the_latest_version` | `S` `a_new_db_starts_at_the_latest_version` |
| `test_a_db_from_before_versions_is_brought_up_to_date` | `S` `a_db_from_before_versions_is_brought_up_to_date` |
| `test_a_version_3_db_gains_the_separated_stems` | `S` `a_version_3_db_gains_the_separated_stems` |
| `test_the_spanish_schema_is_migrated_in_full` | `S` `the_spanish_schema_is_migrated_in_full` |
| `test_a_new_db_skips_the_migrations` | `S` `a_new_db_skips_the_migrations` |
| `test_migrations_are_applied_only_once` | `S` `migrations_are_applied_only_once` |
| `test_a_failing_migration_leaves_nothing_half_done` | `S` `a_failing_migration_leaves_nothing_half_done` |
| `test_a_db_from_a_newer_danplay_is_left_alone` | `S` `a_db_from_a_newer_danplay_is_left_alone` |
| `test_a_with_block_commits_and_closes_the_connection` | `server/src/app.rs` `a_db_block_commits_or_rolls_back` (`App::db` y `App::db_write`, lo que hace ahora el `with`; que la conexión quede cerrada lo da Rust al soltarla, no hay nada que mirar) |
| `test_new_or_migrated_dbs_end_with_the_same_columns` | `S` `new_or_migrated_dbs_end_with_the_same_columns` |

## tests/test_schema_rust.py

Comparaba en vivo el esquema y las migraciones de Rust con los de Python
(danplay/library/db.py). Cómo dejaba Python cada base de partida (las
tablas, índices y triggers, la versión, las filas de cada tabla y lo que
encuentra la búsqueda) se guardó una vez en
`library/src/index/schema/python_cases.json`, y se compara con lo que deja
Rust.

| Prueba de Python | En Rust |
|---|---|
| `test_rust_leaves_the_db_like_python` | `S` `rust_leaves_the_db_like_python` (las cuatro bases de partida) |
| `test_an_up_to_date_db_does_not_change` | `S` `an_up_to_date_db_does_not_change` |
| `test_a_db_from_a_newer_danplay_is_left_alone` | `S` `a_db_from_a_newer_danplay_is_left_alone` (la misma que la de test_schema.py, que mira también que no se aplique nada) |

## tests/test_separation.py

`P` = `analysis/src/separate/tests.rs` (sin ffmpeg, con la red de mentira
`Identity`); `J` = `analysis/src/separate/process/tests.rs` (el trabajo
entero, con ffmpeg y canciones de tonos en wav). ONNX Runtime, para el grafo
de verdad, se busca en `DANPLAY_ONNXRUNTIME`/`ORT_DYLIB_PATH`, en lo que deja
la compilación en `target/<perfil>/data` o en `desktop/src-tauri/data`
(`analysis/src/testkit.rs`); sin él, esa prueba lo dice y no comprueba nada.

| Prueba de Python | En Rust |
|---|---|
| `test_the_spectrogram_and_its_inverse_give_back_the_signal` | ya estaba en `P` `spectrogram_and_inverse_give_back_the_signal` |
| `test_the_pieces_are_joined_back_into_the_whole_song` | ya estaba en `P` `chunks_join_into_the_whole_song` (los tres largos) |
| `test_a_song_with_an_offset_and_a_loud_mix_comes_back_the_same` | `P` `a_song_with_an_offset_and_a_loud_mix_comes_back_the_same` |
| `test_it_stops_when_cancelled` | ya estaba en `P` `stops_on_cancel_and_a_join_error_is_not_lost` |
| `test_a_failure_while_joining_is_not_swallowed` | ya estaba en `P` `stops_on_cancel_and_a_join_error_is_not_lost` |
| `test_each_track_gets_its_wave_on_a_common_scale` | ya estaba en `P` `each_stem_waveform_shares_one_scale` |
| `test_safetensors_are_read_without_the_library` | `P` `safetensors_are_read_without_the_library` |
| `test_the_real_graph_loads_with_weights_laid_out_like_the_official_ones` | `P` `the_real_graph_loads_with_weights_laid_out_like_the_official_ones` (los dos grafos, con ONNX Runtime de verdad; los grafos siguen en `danplay/data/separator`) |
| `test_what_is_not_in_the_song_is_not_a_track_and_all_add_up_to_the_song` | `J` `what_is_not_in_the_song_is_not_a_track_and_all_add_up_to_the_song` (lo que «contó» va en los `Warning` que recibe quien llama, no en líneas de la salida) |
| `test_the_improvement_keeps_what_was_decided` | `J` `the_improvement_keeps_what_was_decided` |
| `test_a_source_that_only_plays_a_moment_stays` | ya estaba en `P` `a_source_heard_only_briefly_is_kept` |
| `test_the_tracks_can_be_kept_in_opus` | `J` `the_tracks_can_be_kept_in_opus` (si ese ffmpeg sabe hacer Opus) |
