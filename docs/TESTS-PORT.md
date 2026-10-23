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
