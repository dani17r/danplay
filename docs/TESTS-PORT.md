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
