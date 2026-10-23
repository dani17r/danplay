# Las pruebas de Python, pasadas a Rust

La fase 5 quita Python del todo. Lo que queda (`danplay/` y `tests/`) solo
sirve de testigo: antes de borrarlo, sus pruebas pasan a Rust, con las mismas
comprobaciones y los mismos valores esperados. Aquí, prueba a prueba, dónde
está cada una ahora:

- una ruta y un nombre: la prueba de Rust que la sustituye;
- «ya estaba en …»: Rust ya comprobaba lo mismo (no se duplica);
- «no se pasa: …»: solo tenía sentido para Python, con el porqué.

Las rutas son de `server/src/` (el crate `danplay-server`) salvo que digan
`analysis/src/` (`danplay-analysis`).

## Servidor (B)

### tests/test_pulse.py (26)

| Python | Rust |
| --- | --- |
| `test_the_spectrogram_has_fifty_frames_a_second_and_hears_the_pitch` | ya estaba en `analysis/src/pulse/tests.rs`: `spectrogram_has_50_frames_per_second_and_hears_pitch` (ahora con las mismas cuentas: la trama 50, la banda 30, la forma de las dos) |
| `test_the_mel_filters_cover_30_hz_to_11_khz` | ya estaba en `analysis/src/pulse/tests.rs`: `filters_span_30_hz_to_11_khz` |
| `test_the_chunks_are_joined_back_frame_by_frame` (4 tamaños) | ya estaba en `analysis/src/pulse/tests.rs`: `chunks_join_frame_by_frame` (ahora también mira que ningún trozo pase de `CHUNK`) |
| `test_the_beats_are_the_peaks_and_each_one_goes_to_its_beat` | ya estaba en `analysis/src/pulse/tests.rs`: `beats_are_peaks_and_downbeats_snap_to_beats` |
| `test_the_grid_keeps_one_level_from_start_to_end` | ya estaba en `analysis/src/pulse/tests.rs`: `grid_stays_on_one_level_from_start_to_end` (ahora también `first_downbeat`; `closed` lo pone siempre el núcleo al guardarla) |
| `test_each_beat_falls_between_frames_where_it_is` | `analysis/src/pulse/tests.rs`: `each_beat_falls_between_frames_where_it_is` |
| `test_a_free_part_has_no_click_and_a_hole_in_tempo_is_filled` | `analysis/src/pulse/tests.rs`: `a_free_part_has_no_click_and_a_hole_in_tempo_is_filled` (lo de dos golpes solos ya estaba en `reports_when_there_is_no_beat`) |
| `test_the_one_stays_put_unless_the_song_says_otherwise` | `analysis/src/pulse/tests.rs`: `the_one_stays_put_unless_the_song_says_otherwise` |
| `test_the_grid_is_worked_out_once_and_kept` | `services/beats/tests.rs`: `the_grid_is_worked_out_once_and_kept` |
| `test_a_song_without_pulse_says_why` | `services/beats/tests.rs`: `a_song_without_pulse_says_why` |
| `test_the_network_says_how_far_it_has_got` | ya estaba en `analysis/src/pulse/tests.rs`: `chunks_join_frame_by_frame` (el avance, trozo a trozo hasta el total, con cada tamaño) |
| `test_the_progress_lines_are_not_taken_for_the_answer` | `services/beats/tests.rs`: `the_progress_lines_are_not_taken_for_the_answer` |
| `test_without_waiting_it_answers_at_once_and_works_it_out_behind` | `services/beats/tests.rs`: `without_waiting_it_answers_at_once_and_works_it_out_behind` |
| `test_what_is_needed_now_goes_before_what_comes_later` | `services/beats/tests.rs`: `what_is_needed_now_goes_before_what_comes_later` |
| `test_a_failure_is_remembered_until_the_file_changes` | `services/beats/tests.rs`: `a_failure_is_remembered_until_the_file_changes` |
| `test_writing_a_tag_keeps_what_was_worked_out_from_the_sound` | `services/beats/tests.rs`: `writing_a_tag_keeps_what_was_worked_out_from_the_sound` |
| `test_the_api_gives_the_grid_or_says_it_cannot` | `services/beats/tests.rs`: `the_api_gives_the_grid_or_says_it_cannot` |
| `test_the_api_without_waiting_and_preparing_the_next` | `services/beats/tests.rs`: `the_api_without_waiting_and_preparing_the_next` |
| `test_the_graph_travels_with_the_app` | `services/beats/tests.rs`: `the_graph_travels_with_the_app` |
| `test_what_the_network_heard_is_kept_and_a_new_version_needs_no_network` | `services/beats/tests.rs`: `what_the_network_heard_is_kept_and_a_new_version_needs_no_network` |
| `test_reanalysing_a_stretch_only_touches_that_stretch` | `services/beats/tests.rs`: `reanalysing_a_stretch_only_touches_that_stretch` |
| `test_steady_tempo_in_a_stretch_fits_between_its_neighbours` | `services/beats/tests.rs`: `steady_tempo_in_a_stretch_fits_between_its_neighbours` |
| `test_the_whole_song_with_the_separated_drums` | `services/beats/tests.rs`: `the_whole_song_with_the_separated_drums` (la batería separada, de verdad en la biblioteca en vez de un `_drums_of` de mentira). Lo de `beatgrid.improve` no se pasa: solo lo usaba la CLI de Python (`danplay/cli.py`), y el núcleo de Rust no lo tiene |
| `test_changes_counts_places_not_beats` | ya estaba en `analysis/src/beatgrid.rs`: `counts_places_not_beats` |
| `test_the_api_reanalyses` | `services/beats/tests.rs`: `the_api_reanalyses` |
| `test_what_the_network_heard_survives_a_cut` | `analysis/src/pulse/tests.rs`: `what_the_network_heard_survives_a_cut` (lo de leer uno roto ya estaba en `net_output_loads_back_unchanged`) |

El pulso es de todo el proceso (una cola, un detector): sus pruebas van una
detrás de otra (`services::beats::tests::serial`, que usa también
`routes::songs::tests::the_click_with_a_fake_detector`). El detector de
mentira es un `sh` que copia lo que «oiría» la red, hecho en Rust. Las
costuras, solo de pruebas: el hilo de la cola parado (`Paused`, lo que en
Python era `with beatgrid._WAKE`) y el grafo de mentira (`set_graph`).

### tests/test_terms.py (4)

| Python | Rust |
| --- | --- |
| `test_at_first_nothing_was_accepted` | `api_tests/terms.rs`: `at_first_nothing_was_accepted` |
| `test_accepting_is_remembered` | `api_tests/terms.rs`: `accepting_is_remembered` (`lib.rs`: `tests::terms` ya miraba que se acepta, sin la versión de la app, el archivo ni la de después) |
| `test_a_strange_version_is_refused` (5 versiones) | `api_tests/terms.rs`: `a_strange_version_is_refused` |
| `test_a_broken_file_counts_as_not_accepted` | `api_tests/terms.rs`: `a_broken_file_counts_as_not_accepted` |

### tests/test_jobs.py (8)

| Python | Rust |
| --- | --- |
| `test_a_job_reports_its_progress_and_leaves_its_result` | `jobs.rs`: `tests::a_job_reports_its_progress_and_leaves_its_result` (las claves de la instantánea ya estaban en `tests::a_job_runs_reports_and_ends`) |
| `test_the_snapshot_is_a_copy` | `jobs.rs`: `tests::the_snapshot_is_a_copy` |
| `test_errors_come_out_in_spanish` | ya estaba en `jobs.rs`: `tests::errors_read_like_python` (el previsto tal cual, el otro con «failure: motivo», sin resultado) |
| `test_finished_jobs_are_forgotten_after_a_while` | `jobs.rs`: `tests::finished_jobs_are_forgotten_after_a_while` (mirando pasado el `TTL`, en vez de ponerlo a cero) |
| `test_the_status_carries_a_copy_of_the_jobs` | `api_tests/jobs.rs`: `the_status_carries_a_copy_of_the_jobs` |
| `test_asking_for_a_scan_while_one_runs_returns_the_same` | `api_tests/jobs.rs`: `asking_for_a_scan_while_one_runs_returns_the_same` (el escaneo, frenado con su cerrojo en vez de con un `library.scan` de mentira) |
| `test_a_real_convert_is_a_job_and_a_dry_run_is_not` | `api_tests/jobs.rs`: `a_real_convert_is_a_job_and_a_dry_run_is_not` (con una conversión de verdad, un wav a mp3, en vez de la de mentira: por eso `converted` es 1 y el `dry_run` está en cada archivo) |
| `test_import_is_a_job_that_counts_each_file` | `api_tests/jobs.rs`: `import_is_a_job_that_counts_each_file` |

### tests/test_tidy.py (5)

| Python | Rust |
| --- | --- |
| `test_it_proposes_only_what_is_out_of_place_and_never_your_own_folders` | `api_tests/tidy.rs`: `it_proposes_only_what_is_out_of_place_and_never_your_own_folders` |
| `test_applying_keeps_the_same_song_and_tidies_the_empty_folder` | `api_tests/tidy.rs`: `applying_keeps_the_same_song_and_tidies_the_empty_folder` |
| `test_corrected_by_hand_it_goes_to_the_tags_too` | `api_tests/tidy.rs`: `corrected_by_hand_it_goes_to_the_tags_too` |
| `test_what_was_worked_out_from_the_sound_follows_the_song` | `api_tests/tidy.rs`: `what_was_worked_out_from_the_sound_follows_the_song` |
| `test_the_api_lists_applies_and_refuses_paths_outside` | `api_tests/tidy.rs`: `the_api_lists_applies_and_refuses_paths_outside` (`routes/songs/tests.rs`: `tidy_and_move_keep_the_same_song` ya miraba una parte: aplicar y la ruta de fuera) |

### tests/test_sequence.py (6)

| Python | Rust |
| --- | --- |
| `test_what_the_folder_says` | `api_tests/sequence.rs`: `what_the_folder_says` |
| `test_the_names_of_downloaded_sequences` | `api_tests/sequence.rs`: `the_names_of_downloaded_sequences` |
| `test_a_project_with_entities_is_not_read` | `api_tests/sequence.rs`: `a_project_with_entities_is_not_read` |
| `test_the_exact_grid_follows_the_meter_changes` | `api_tests/sequence.rs`: `the_exact_grid_follows_the_meter_changes` |
| `test_opening_it_leaves_it_ready_to_play` | `api_tests/sequence.rs`: `opening_it_leaves_it_ready_to_play` (por el trabajo «sequence», que da el avance; lo de que el clic exacto vale con otra `VERSION`, con lo guardado de otra versión en vez de cambiar la constante) |
| `test_the_api_opens_it` | `api_tests/sequence.rs`: `the_api_opens_it` |

### tests/test_journal.py (10)

| Python | Rust |
| --- | --- |
| `test_a_song_that_goes_stays_in_its_list_as_missing` | `api_tests/journal.rs`: `a_song_that_goes_stays_in_its_list_as_missing` |
| `test_it_comes_back_in_another_file_to_its_place` | `api_tests/journal.rs`: `it_comes_back_in_another_file_to_its_place` |
| `test_a_missing_one_keeps_its_place_when_the_list_is_reordered` | `api_tests/journal.rs`: `a_missing_one_keeps_its_place_when_the_list_is_reordered` |
| `test_deleting_it_from_danplay_can_take_it_out_of_its_lists` | `api_tests/journal.rs`: `deleting_it_from_danplay_can_take_it_out_of_its_lists` (a la papelera de las pruebas en vez de un `trash_path` de mentira) |
| `test_the_api_deletes_from_the_lists_too` | `api_tests/journal.rs`: `the_api_deletes_from_the_lists_too` (`routes/songs/tests.rs`: `to_the_trash_and_out_of_the_lists` ya miraba el `lists`, sin la lista vacía después) |
| `test_the_diary_is_written_next_to_the_music` | `api_tests/journal.rs`: `the_diary_is_written_next_to_the_music` |
| `test_the_diary_of_before_1_31_is_still_read_and_then_replaced` | `api_tests/journal.rs`: `the_diary_of_before_1_31_is_still_read_and_then_replaced` (lo escrito, olvidado con otra app sobre los mismos datos en vez de tocar `_STATE`) |
| `test_a_diary_written_on_windows_is_understood_anywhere` | `api_tests/journal.rs`: `a_diary_written_on_windows_is_understood_anywhere` |
| `test_a_lost_database_is_rebuilt_from_the_diary` | `api_tests/journal.rs`: `a_lost_database_is_rebuilt_from_the_diary` |
| `test_moving_the_music_keeps_the_lists_and_does_not_undo_what_you_removed` | `api_tests/journal.rs`: `moving_the_music_keeps_the_lists_and_does_not_undo_what_you_removed` |

### tests/test_watch.py (23)

| Python | Rust |
| --- | --- |
| `test_a_song_moved_to_another_folder_keeps_its_id_and_its_lists` | `api_tests/watch.rs`: `a_song_moved_to_another_folder_keeps_its_id_and_its_lists` |
| `test_a_moved_song_never_disappears_halfway` | `api_tests/watch.rs`: `a_moved_song_never_disappears_halfway` |
| `test_a_song_renamed_in_place_keeps_its_id` | `api_tests/watch.rs`: `a_song_renamed_in_place_keeps_its_id` |
| `test_a_deleted_song_leaves_the_views_and_stays_in_its_lists_as_missing` | `api_tests/watch.rs`: `a_deleted_song_leaves_the_views_and_stays_in_its_lists_as_missing` |
| `test_a_song_that_comes_back_returns_to_its_place_in_the_list` | `api_tests/watch.rs`: `a_song_that_comes_back_returns_to_its_place_in_the_list` |
| `test_new_songs_never_inherit_the_id_of_one_that_left` | `api_tests/watch.rs`: `new_songs_never_inherit_the_id_of_one_that_left` |
| `test_songs_that_never_come_back_are_forgotten_after_a_while` | `api_tests/watch.rs`: `songs_that_never_come_back_are_forgotten_after_a_while` |
| `test_a_scan_that_finds_nothing_new_does_not_refresh_the_app` | `api_tests/watch.rs`: `a_scan_that_finds_nothing_new_does_not_refresh_the_app` (la revisión es de todo el proceso y otras pruebas la mueven a la vez: se intenta hasta que un escaneo la deja quieta; si el escaneo la moviera, no lo haría nunca) |
| `test_a_name_that_is_not_utf8_does_not_stop_the_scan` | `api_tests/watch.rs`: `a_name_that_is_not_utf8_does_not_stop_the_scan` |
| `test_a_folder_that_moved_hides_its_songs_and_says_so` | `api_tests/watch.rs`: `a_folder_that_moved_hides_its_songs_and_says_so` |
| `test_choosing_the_moved_folder_brings_everything_back` | `api_tests/watch.rs`: `choosing_the_moved_folder_brings_everything_back` |
| `test_relocating_by_hand` | `api_tests/watch.rs`: `relocating_by_hand` |
| `test_an_unrelated_folder_is_not_taken_for_the_moved_one` | `api_tests/watch.rs`: `an_unrelated_folder_is_not_taken_for_the_moved_one` |
| `test_a_disk_that_comes_back_brings_its_songs_as_they_were` | `api_tests/watch.rs`: `a_disk_that_comes_back_brings_its_songs_as_they_were` |
| `test_removing_a_folder_and_adding_it_again_keeps_the_ids` | `api_tests/watch.rs`: `removing_a_folder_and_adding_it_again_keeps_the_ids` |
| `test_locate_says_where_each_song_is_now` | `api_tests/watch.rs`: `locate_says_where_each_song_is_now` |
| `test_the_watcher_follows_what_happens_on_disk` | ya estaba en `services/watcher/tests.rs`: `the_watcher_follows_what_happens_on_disk` |
| `test_a_folder_added_in_settings_is_indexed_without_asking` | ya estaba en `services/watcher/tests.rs`: `a_folder_added_in_settings_is_indexed_without_asking` |
| `test_the_watcher_ignores_what_does_not_matter` | ya estaba en `services/watcher/tests.rs`: `the_watcher_ignores_what_does_not_matter` (con los avisos de `notify` en vez de los de watchdog) |
| `test_the_watcher_waits_for_calm_before_scanning` | ya estaba en `services/watcher/tests.rs`: `the_watcher_waits_for_calm_before_scanning` |
| `test_prepare_hides_a_missing_folder_before_the_first_answer` | ya estaba en `services/watcher/tests.rs`: `prepare_hides_a_missing_folder_before_the_first_answer` |
| `test_nothing_brings_a_moved_folder_back_empty` | `api_tests/watch.rs`: `nothing_brings_a_moved_folder_back_empty` (con un yt-dlp de mentira en `sh`, para ver que no llega a bajar nada) |
| `test_a_new_inbox_is_still_created_where_nothing_is_missing` | `api_tests/watch.rs`: `a_new_inbox_is_still_created_where_nothing_is_missing` |

### tests/test_index.py (14)

| Python | Rust |
| --- | --- |
| `test_without_tags_the_file_name_gives_the_artist` | `api_tests/index.rs`: `without_tags_the_file_name_gives_the_artist` |
| `test_several_dashes_and_feat` | `api_tests/index.rs`: `several_dashes_and_feat` |
| `test_the_tag_and_folder_win_over_the_file_name` | `api_tests/index.rs`: `the_tag_and_folder_win_over_the_file_name` |
| `test_the_api_search_is_light_and_counts` | `api_tests/index.rs`: `the_api_search_is_light_and_counts` |
| `test_playlists_and_the_player_are_light_too` | `api_tests/index.rs`: `playlists_and_the_player_are_light_too` |
| `test_an_odd_number_in_a_search_does_not_break_the_query` | `api_tests/index.rs`: `an_odd_number_in_a_search_does_not_break_the_query` |
| `test_the_stats_do_not_keep_a_total_from_before_a_change` | `api_tests/index.rs`: `the_stats_do_not_keep_a_total_from_before_a_change` (el «otro hilo que escribe» va entre leer la revisión y contar, en vez de en un `connect` de mentira; lo de que sin cambios vale se intenta hasta que nadie se cruza, que la caché y la revisión son de todo el proceso) |
| `test_the_revision_loses_no_bumps_with_many_threads` | `api_tests/index.rs`: `the_revision_loses_no_bumps_with_many_threads` (otras pruebas la mueven a la vez: nunca menos de lo nuestro, y alguna vez justo lo nuestro) |
| `test_only_audio_is_opened` | `api_tests/index.rs`: `only_audio_is_opened` (lo del 404 ya estaba en `api_tests/external.rs`: `saving_dropping_and_missing_files`) |
| `test_the_player_list_mixes_both_sources_in_order` | `api_tests/index.rs`: `the_player_list_mixes_both_sources_in_order` |
| `test_finding_one_repertoire_does_not_recompute_all` | `api_tests/index.rs`: `finding_one_repertoire_does_not_recompute_all` (lo que encuentra; lo de que no pide la lista entera no se pasa: en Rust `by_name` es una consulta suya y no hay `list_all` que sustituir. Y `by_id("x")` tampoco: el id es un `i64`) |
| `test_the_playlist_songs_index_exists` | `api_tests/index.rs`: `the_playlist_songs_index_exists` |
| `test_thumbnails_are_pruned_by_the_scan` | `api_tests/index.rs`: `thumbnails_are_pruned_by_the_scan` |
| `test_the_thumbnail_cache_has_a_cap` | `api_tests/index.rs`: `the_thumbnail_cache_has_a_cap` |

### tests/test_privacy.py (17)

| Python | Rust |
| --- | --- |
| `test_the_tests_do_not_touch_the_real_data` | `api_tests/privacy.rs`: `the_tests_do_not_touch_the_real_data` (lo que da `tests::app()`: todo en su temporal, nada en las carpetas de verdad, sin el .env del proyecto) |
| `test_the_tests_do_not_reach_the_internet` | no se pasa: miraba el corte de red que ponía conftest.py en todo el proceso de Python (un `socket` de mentira). En Rust no hay tal corte: las pruebas no salen porque hablan con servidores en 127.0.0.1 (`FakeAi`) y con programas de mentira (`sh`) |
| `test_localhost_still_works` | no se pasa: la otra mitad del corte de red de conftest.py (que 127.0.0.1 siguiera abierto). Las de la IA de mentira (`api_tests/ai.rs`, `chat.rs`, `assistant.rs`) ya hablan por 127.0.0.1 |
| `test_the_data_folders_are_user_only` | `api_tests/privacy.rs`: `the_data_folders_are_user_only` |
| `test_an_existing_folder_is_locked_down` | `api_tests/privacy.rs`: `an_existing_folder_is_locked_down` |
| `test_the_db_is_created_and_stays_user_only` | `api_tests/privacy.rs`: `the_db_is_created_and_stays_user_only` |
| `test_secrets_are_written_atomically_and_privately` | `api_tests/privacy.rs`: `secrets_are_written_atomically_and_privately` |
| `test_a_failure_midway_leaves_the_previous_file` | `api_tests/privacy.rs`: `a_failure_midway_leaves_the_previous_file` (falla de verdad el cambio de nombre, porque en el sitio hay una carpeta con lo suyo, en vez de un `os.replace` de mentira) |
| `test_settings_read_back_as_they_were_saved` (9 valores) | `api_tests/privacy.rs`: `settings_read_back_as_they_were_saved` (releído con `library::config::dotenv::parse`, lo que hace python-dotenv) |
| `test_a_value_with_a_newline_does_not_invent_variables` | `api_tests/privacy.rs`: `a_value_with_a_newline_does_not_invent_variables` |
| `test_the_migration_from_melodia_respects_xdg` | `api_tests/privacy.rs`: `the_migration_from_melodia_respects_xdg` (el proceso aparte es el mismo programa de pruebas con solo `config_child`, que lee la configuración como al arrancar) |
| `test_profiles_are_saved_for_the_user_only` | `api_tests/privacy.rs`: `profiles_are_saved_for_the_user_only` |
| `test_the_saved_key_does_not_travel_to_another_url` | `api_tests/privacy.rs`: `the_saved_key_does_not_travel_to_another_url` |
| `test_headers_and_extra_go_out_masked_and_come_back_whole` | `api_tests/privacy.rs`: `headers_and_extra_go_out_masked_and_come_back_whole` |
| `test_a_broken_ai_json_is_set_aside_not_overwritten` | `api_tests/privacy.rs`: `a_broken_ai_json_is_set_aside_not_overwritten` |
| `test_saving_profiles_at_once_loses_none` | `api_tests/privacy.rs`: `saving_profiles_at_once_loses_none` |
| `test_folders_set_by_hand_migrate_nothing` | `api_tests/privacy.rs`: `folders_set_by_hand_migrate_nothing` (con `config_child`, como la de melodía) |

### tests/test_cues.py (12)

| Python | Rust |
| --- | --- |
| `test_numbers_and_shorthand_are_said_in_words` | `api_tests/cues.rs`: `numbers_and_shorthand_are_said_in_words` (`library/src/speech.rs` ya tenía otros casos, no estos) |
| `test_the_phonemes_are_the_ones_the_spanish_voices_learnt` | `api_tests/cues.rs`: `the_phonemes_are_the_ones_the_spanish_voices_learnt` |
| `test_polish_trims_the_silence_and_levels_it` | ya estaba en `analysis/src/cues.rs`: `polish_trims_silence_and_sets_level` |
| `test_a_recording_becomes_a_guide_and_only_guides_are_read` | `api_tests/cues.rs`: `a_recording_becomes_a_guide_and_only_guides_are_read` |
| `test_an_imported_audio_lasts_at_most_the_song` | `api_tests/cues.rs`: `an_imported_audio_lasts_at_most_the_song` (lo de «como mucho», con un audio que pasa del `MAX_IMPORT_SECONDS` de verdad en vez de bajarlo a 50) |
| `test_a_guide_is_cut_faded_and_cleaned_into_a_new_one` | `api_tests/cues.rs`: `a_guide_is_cut_faded_and_cleaned_into_a_new_one` (lo de quitar el ruido y los graves, con ffmpeg de verdad: la cadena exacta de filtros no se ve desde fuera) |
| `test_cleaning_a_guide_with_ffmpeg` | `api_tests/cues.rs`: `cleaning_a_guide_with_ffmpeg` (por `edit_clip`, que es quien llama a ffmpeg; el ruido, de un generador propio) |
| `test_the_voices_and_what_they_weigh` | `services/cues.rs`: `tests::the_voices_and_what_they_weigh` |
| `test_installing_a_voice_checks_what_arrives` | `services/cues.rs`: `tests::installing_a_voice_checks_what_arrives` (un servidor de archivos en 127.0.0.1 en vez de un `urlopen` de mentira, y archivos más ligeros que la voz de verdad) |
| `test_the_api_for_guides` | `api_tests/cues.rs`: `the_api_for_guides` (con la voz de mentira, `services::cues::fake_voice`; lo de `available` depende de si el equipo tiene ONNX Runtime: en Python su entorno lo tenía) |
| `test_the_old_and_the_new_name_are_merged` | ya estaba en `startup.rs`: `tests::the_old_and_the_new_name_are_merged` |
| `test_the_guides_of_before_1_31_move_to_their_new_folder` | `api_tests/cues.rs`: `the_guides_of_before_1_31_move_to_their_new_folder` (lo de la base, al arrancar otra app sobre ella) |

La voz de mentira es una costura solo de pruebas en `services/cues.rs`
(`fake_voice`, por app): lo que en Python eran `cues.say`, `cues.installed`
y `cues.install` sustituidos. Dice medio segundo de un tono, como la de
Python, y no baja nada.

### tests/test_structure.py (12)

| Python | Rust |
| --- | --- |
| `test_the_parts_come_out_with_their_names` | `analysis/src/structure.rs`: `tests::the_parts_come_out_with_their_names` |
| `test_a_solo_is_named_by_the_instrument_that_leads` | `analysis/src/structure.rs`: `tests::a_solo_is_named_by_the_instrument_that_leads` |
| `test_what_is_left_before_the_first_chorus_is_the_verse` | `analysis/src/structure.rs`: `tests::what_is_left_before_the_first_chorus_is_the_verse` |
| `test_the_voice_also_marks_a_change_and_short_parts_join_a_neighbour` | `analysis/src/structure.rs`: `tests::the_voice_also_marks_a_change_and_short_parts_join_a_neighbour` |
| `test_the_units_are_the_bars_or_two_second_windows` | `analysis/src/structure.rs`: `tests::the_units_are_the_bars_or_two_second_windows` (lo de pocos compases ya estaba en `tests::no_bars_means_two_second_chunks`) |
| `test_the_frames_hear_the_notes` | `analysis/src/structure.rs`: `tests::the_frames_hear_the_notes` |
| `test_each_guide_ends_a_beat_before_its_part_and_none_overlap` | `analysis/src/structure.rs`: `tests::each_guide_ends_a_beat_before_its_part_and_none_overlap` (`tests::cue_ends_one_beat_before_its_section` ya miraba el caso sencillo) |
| `test_the_automatic_guide_finds_the_parts_and_says_them` | `api_tests/structure.rs`: `the_automatic_guide_finds_the_parts_and_says_them` |
| `test_with_its_stems_the_intro_and_the_end_are_where_nobody_sings` | `api_tests/structure.rs`: `with_its_stems_the_intro_and_the_end_are_where_nobody_sings` |
| `test_named_parts_are_used_as_they_are` | `api_tests/structure.rs`: `named_parts_are_used_as_they_are` (lo que sale; lo de que no se escucha la canción no se pasa: en Python era un `structure.analyse` de mentira que fallaba si se llamaba, y en Rust no hay por dónde cambiarlo) |
| `test_without_the_voice_it_starts_downloading_it` | `api_tests/structure.rs`: `without_the_voice_it_starts_downloading_it` |
| `test_a_song_too_short_has_no_parts` | `api_tests/structure.rs`: `a_song_too_short_has_no_parts` |

Los rasgos sintéticos salen de un generador propio (splitmix64, con las
mismas semillas 1, 3 y 5) en vez del de numpy: el mismo reparto (uniforme y
normal), otros números; con ellos salen las mismas partes y los mismos
nombres. La canción de punta a punta, igual que la de Python (el ruido del
charles, de otro generador).
