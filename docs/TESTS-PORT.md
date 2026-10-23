# Las pruebas de Python, pasadas a Rust

La fase 5 quita Python del todo. Lo que queda (`danplay/` y `tests/`) solo
sirve de testigo: antes de borrarlo, sus pruebas pasan a Rust, con las mismas
comprobaciones y los mismos valores esperados. Aquí, prueba a prueba, dónde
está cada una ahora:

- una ruta y un nombre: la prueba de Rust que la sustituye;
- «ya estaba en …»: Rust ya comprobaba lo mismo (no se duplica);
- «no se pasa: …»: solo tenía sentido para Python, con el porqué.

Las rutas son de `server/src/` (el crate `danplay-server`) salvo que digan
`analysis/src/` (`danplay-analysis`), `library/src/` (`danplay-library`) o
`server/tests/` (las de integración de `danplay-server`).

## Servidor (B)

De las 137 pruebas de estos once archivos: 120 nuevas en Rust, 15 que ya
estaban (con lo que les faltaba añadido) y 2 que no se pasan (las del corte
de red de conftest.py). Trozos sueltos que no se pasan, cada uno dicho en su
fila: `beatgrid.improve` (solo de la CLI de Python), que `by_name` no pida
la lista entera y `by_id("x")`, que con las partes puestas no se escuche la
canción, y la cadena exacta de filtros de ffmpeg de una guía.

Costuras nuevas, todas solo de pruebas (`#[cfg(test)]`): en `services/beats.rs`
el hilo de la cola parado y el grafo de mentira; en `services/cues.rs` la voz
de mentira (por app). Y una diferencia real, arreglada en Rust: la de la cola
del pulso (abajo, en test_pulse.py).

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

Lo que salió: el hilo de la cola sacaba la canción de la cola y la marcaba
como «calculándose» con dos cerrojos distintos. En ese momento no estaba en
ningún sitio: `idle` decía que ya no quedaba nada, `progress` que no se
había pedido, y pedirla otra vez la volvía a poner en cola (y se calculaba
dos veces). En Python las dos cosas iban con el mismo cerrojo (`_WAKE`);
ahora en Rust también (`beats::work`).

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

## Servidor (A)

De las 359 pruebas de estos doce archivos: 313 nuevas en Rust, 14 que ya
estaban y 32 que no se pasan. No se pasan la línea de
órdenes de Python (test_cli.py), lo de Windows y del paquete de PyInstaller
(test_no_console.py, test_system_libs.py), lo de PyPI de test_ytdlp.py, tres
de test_serve.py y tres de test_youtube_after.py (la excepción sin texto y
dos de `after`).

Trozos sueltos que no se pasan, cada uno dicho en su fila: los contadores de
los espías del plan (`stamps`, `spy_place`), su 503 con yt-dlp en el PATH y
que el timeout de `info` vaya por su nombre. Y huecos pequeños, también en su
fila («no mira»), entre ellos: el respaldo por ffmpeg de la forma de onda;
buscar los cifrados en internet, `refresh` y el aviso de no dar acordes de
memoria; que sin token no se escuche fuera; en el paquete del celular, el
`click_error` de una canción sin pulso y su `version`; en la mezcla por
lados, la derecha callada tras la guía y el 422 de un lado «C»; y
`/api/youtube` con la versión de yt-dlp y su puesta al día como trabajo.

Lo que destaparon y se arregló en Rust:

- Con color, un «ERROR:» de yt-dlp no se reconocía y el motivo se quedaba en
  «yt-dlp termino con el codigo 1». Ahora `Stderr::line` (`services/ytdlp.rs`)
  mira la línea sin color (`library::youtube::uncolored`).
- Una base borrada por fuera con la app abierta se volvía a crear vacía, y
  todo fallaba con «no such table». Ahora `library/src/index/schema.rs`:
  `connect` la prepara otra vez.
- Las pruebas del plan, del registro y de un turno del chat compartían el
  registro de propuestas sin cerrojo común. Ahora van con `testkit::registry()`,
  reentrante, y toda IA de mentira lo coge (el estado de la IA es uno para el
  proceso).
- En las pruebas, el plan ya no busca con el yt-dlp que haya en el PATH: sin
  una consulta de mentira contesta `plan::testing::OFFLINE`.

Costuras nuevas, todas solo de pruebas (`#[cfg(test)]`): `plan::testing` en
`services/chat/plan.rs` (`CLOCK`, `INFO`, `WARM`, `MINE`, `SEEN`);
`youtube::testing` en `services/youtube.rs` (la descarga de mentira, por app,
y lo que había al soltar cada turno); en `services/stems.rs`, el motor de
mentira (`Separator::set_engine`) y lo que dice ffmpeg de sus códecs
(`stems::set_encoders`); `media::cover_cache_bytes` y `bundle::Failure::text`.
Y pasan a `pub(crate)`, para llamarlas desde las pruebas: `chat::still_there`,
`bundle::export` y `stems::installed`.

### tests/test_api.py (150)

| Python | Rust |
| --- | --- |
| `test_status` | `api_tests/library.rs`: `status` |
| `test_search_everything` | `api_tests/library.rs`: `search_everything_by_artist_duration_and_nothing` |
| `test_search_by_artist` | `api_tests/library.rs`: `search_everything_by_artist_duration_and_nothing` |
| `test_search_duration_comparator` | `api_tests/library.rs`: `search_everything_by_artist_duration_and_nothing` |
| `test_search_with_no_results` | `api_tests/library.rs`: `search_everything_by_artist_duration_and_nothing` |
| `test_search_ignores_lone_punctuation_and_knows_title_filter` | `api_tests/library.rs`: `search_ignores_lone_punctuation_and_knows_the_title_filter` |
| `test_facets` | `api_tests/library.rs`: `facets` |
| `test_song_and_edit` | `api_tests/library.rs`: `a_song_and_editing_it` |
| `test_missing_song` | `api_tests/library.rs`: `a_song_and_editing_it` (el 404, al final) |
| `test_stars_and_favorite` | `api_tests/library.rs`: `stars_and_favorite` |
| `test_audio_is_served` | `api_tests/library.rs`: `the_audio_its_path_and_the_cover` |
| `test_path_for_rust` | `api_tests/library.rs`: `the_audio_its_path_and_the_cover` |
| `test_cover_without_artwork_gives_404` | `api_tests/library.rs`: `the_audio_its_path_and_the_cover` |
| `test_playlist_lifecycle` | `api_tests/library.rs`: `a_playlist_from_start_to_end` |
| `test_folders_and_exclusions` | `api_tests/library.rs`: `folders_and_exclusions` |
| `test_adding_missing_folder_gives_400` | `api_tests/library.rs`: `folders_and_exclusions` (el 400, al final) |
| `test_transpose_endpoint` | `api_tests/library.rs`: `transpose` (`routes/songs/tests.rs`: `transposing_text_and_the_stored_sheet` ya miraba otros casos) |
| `test_inbox_and_convertible` | `api_tests/library.rs`: `the_inbox_what_can_be_converted_and_a_dry_run_import` |
| `test_dry_run_import_moves_nothing` | `api_tests/library.rs`: `the_inbox_what_can_be_converted_and_a_dry_run_import` |
| `test_duplicates` | `api_tests/library.rs`: `duplicates` |
| `test_settings_round_trip` | `api_tests/ai.rs`: `settings_round_trip_the_stems_format_and_the_key` |
| `test_the_stems_format_is_a_setting` | `api_tests/ai.rs`: `settings_round_trip_the_stems_format_and_the_key` (el «flac» de partida lo pone la API, sin un `config.STEMS_FORMAT` de mentira) |
| `test_ai_key_is_never_returned_whole` | `api_tests/ai.rs`: `settings_round_trip_the_stems_format_and_the_key` |
| `test_ai_providers_overview` | `api_tests/ai.rs`: `the_ai_providers_overview` |
| `test_ai_profile_save_activate_and_delete` | `api_tests/ai.rs`: `an_ai_profile_saved_activated_and_deleted` |
| `test_ai_check_and_models_with_a_fake_provider` | `api_tests/ai.rs`: `checking_and_listing_models_with_a_fake_provider` (la IA de mentira, un servidor en 127.0.0.1 con su `base_url` en el borrador, en vez de un `ai._build_client` de mentira) |
| `test_chat_start_poll_and_cancel` | `api_tests/ai.rs`: `the_live_chat_starts_answers_and_cancels` (con `FakeAi::scripted`; el perfil apunta a ella con `base_url`) |
| `test_chats_are_kept_and_searchable` | `api_tests/ai.rs`: `chats_are_kept_and_searchable` |
| `test_ai_usage_and_fallback_settings` | `api_tests/ai.rs`: `ai_usage_and_the_fallback_setting` |
| `test_study_mode_is_saved_in_the_index_and_in_the_file` | `api_tests/songs.rs`: `the_study_mode_is_saved_in_the_index_and_in_the_file` (`routes/songs/tests.rs`: `study_validates_like_pydantic_and_saves_it_clean` ya miraba la validación y el vacío) |
| `test_waveform_columns_follow_the_sound` | ya estaba en `library/src/waveform/tests.rs`: `columns_follow_the_sound` |
| `test_waveform_endpoint_draws_the_song_and_caches_it` | `api_tests/songs.rs`: `the_waveform_draws_the_song_and_keeps_it` (el mp3, callado y luego con tono, de una vez con `aevalsrc` de ffmpeg. No mira que el respaldo por ffmpeg, `waveform::with_ffmpeg`, dé lo mismo) |
| `test_waveform_cache_goes_away_with_the_song` | `api_tests/songs.rs`: `the_waveform_cache_goes_away_with_the_song` (`services::songs::forget` hace de `library.forget`) |
| `test_playlist_sheet_is_written_in_the_playlists_folder` | `api_tests/songs.rs`: `a_playlist_sheet_is_written_in_the_playlists_folder` |
| `test_ai_free_activates_a_keyless_provider` | `library/src/ai/call/tests.rs`: `try_free_activates_the_first_that_answers` y `try_free_with_none_alive_says_so_activates_none` (`call::try_free`, que es lo que hace `POST /api/ai/free`, contra servidores de mentira) |
| `test_indexes_nothing_without_folders` | `api_tests/downloads.rs`: `nothing_is_indexed_without_folders` |
| `test_status_warns_when_no_folders` | `api_tests/folders.rs`: `checking_a_folder_warns_of_inside_same_and_copy` (el `configured` de `/api/status`, al principio) |
| `test_warns_if_folder_is_inside_another` | `api_tests/folders.rs`: `checking_a_folder_warns_of_inside_same_and_copy` |
| `test_warns_if_folder_was_already_there` | `api_tests/folders.rs`: `checking_a_folder_warns_of_inside_same_and_copy` |
| `test_finds_a_copy_of_the_same_music` | `api_tests/folders.rs`: `checking_a_folder_warns_of_inside_same_and_copy` |
| `test_unrelated_folder_does_not_warn` | `api_tests/folders.rs`: `checking_a_folder_warns_of_inside_same_and_copy` |
| `test_adding_the_same_folder_twice_does_not_duplicate` | `api_tests/folders.rs`: `adding_the_same_folder_twice_or_one_inside_changes_nothing` |
| `test_subfolder_of_indexed_is_not_added` | `api_tests/folders.rs`: `adding_the_same_folder_twice_or_one_inside_changes_nothing` |
| `test_parent_folder_replaces_inner_one` | `api_tests/folders.rs`: `a_parent_folder_replaces_the_inner_one` |
| `test_a_copy_asks_for_confirmation_and_can_be_forced` | `api_tests/folders.rs`: `a_copy_asks_for_confirmation_and_can_be_forced` |
| `test_exclusions_ignore_case` | `api_tests/folders.rs`: `exclusions_ignore_case` |
| `test_chat_tools_are_declared` | `api_tests/chat.rs`: `the_tools_are_declared` |
| `test_chat_requires_messages` | `api_tests/chat.rs`: `the_tools_are_declared` |
| `test_chat_search_tool` | `api_tests/chat.rs`: `searching_making_a_playlist_and_transposing` |
| `test_chat_create_playlist_tool` | `api_tests/chat.rs`: `searching_making_a_playlist_and_transposing` (la lista, mirada por `/api/playlists`) |
| `test_chat_transpose_tool` | `api_tests/chat.rs`: `searching_making_a_playlist_and_transposing` |
| `test_chat_chords_tool_says_where_they_come_from` | ya estaba en `library/src/assistant/tools/tests.rs`: `each_tool_says_what_it_did` (las partes, «el cifrado no dice el tono» y el motivo de un fallo de red, con un buscador de mentira; que se guarde en la canción, en `library/src/enrich/tests.rs`: `chords_are_saved_unless_the_network_failed`). No mira el aviso «No des acordes ni tono de memoria» sin cifrado: en el servidor no hay por dónde poner un `chord_sheets.find` de mentira |
| `test_chords_endpoints` | ya estaba en `routes/songs/tests.rs`: `transposing_text_and_the_stored_sheet` (el 404 sin cifrado, transponer el guardado y darlo sin buscar) y en `library/src/enrich/tests.rs`: `chords_are_saved_unless_the_network_failed` (se guarda y la vez siguiente sale `cached`). No mira buscarlo, `refresh` ni el 404 de una canción que no existe: en Python ya era solo de su núcleo, y en Rust el buscador sale a internet sin costura |
| `test_unknown_tool_does_not_blow_up` | `api_tests/chat.rs`: `searching_making_a_playlist_and_transposing` |
| `test_download_without_items_skips_network` | `api_tests/chat.rs`: `downloading_nothing_does_not_start_a_download` (por `/api/youtube` en vez de `YT.STATE`) |
| `test_system_prompt_only_talks_music` | `api_tests/chat.rs`: `the_system_prompt_only_talks_music_and_never_downloads_alone` |
| `test_system_prompt_never_downloads_alone` | `api_tests/chat.rs`: `the_system_prompt_only_talks_music_and_never_downloads_alone` |
| `test_the_control_tools_are_declared` | `api_tests/chat.rs`: `the_tools_are_declared` |
| `test_play_returns_a_command_for_the_app` | `api_tests/chat.rs`: `playing_returns_a_command_for_the_app` |
| `test_playing_something_missing_does_not_crash` | `api_tests/chat.rs`: `playing_returns_a_command_for_the_app` |
| `test_player_control_validates_the_command` | `api_tests/chat.rs`: `playing_returns_a_command_for_the_app` |
| `test_the_assistant_rates_and_marks_favorite` | `api_tests/chat.rs`: `the_assistant_rates_marks_favorites_and_edits` (lo guardado, leído por `/api/song/{id}`) |
| `test_the_assistant_edits_song_data` | `api_tests/chat.rs`: `the_assistant_rates_marks_favorites_and_edits` |
| `test_downloads_enter_the_index_without_rescanning` | `api_tests/folders.rs`: `downloads_enter_the_index_without_rescanning` |
| `test_indexing_outside_the_folders_does_nothing` | `api_tests/folders.rs`: `downloads_enter_the_index_without_rescanning` |
| `test_history_starts_empty_and_accepts_entries` | `api_tests/folders.rs`: `the_download_history` |
| `test_tells_who_asked_for_the_download` | `api_tests/folders.rs`: `the_download_history` |
| `test_stores_the_three_outcomes` | `api_tests/folders.rs`: `the_download_history` |
| `test_history_comes_out_newest_first` | `api_tests/folders.rs`: `the_download_history` |
| `test_the_scan_does_not_erase_the_history` | `api_tests/folders.rs`: `the_download_history` (el escaneo, por `/api/scan`) |
| `test_the_endpoint_returns_the_history` | `api_tests/folders.rs`: `the_download_history` |
| `test_favorites_filter_is_not_cut_off_by_the_limit` | `api_tests/folders.rs`: `the_favorites_and_stars_filters_are_not_cut_off_by_the_limit` |
| `test_min_stars_filter_is_not_cut_off_by_the_limit` | `api_tests/folders.rs`: `the_favorites_and_stars_filters_are_not_cut_off_by_the_limit` |
| `test_search_survives_a_quote_in_the_query` | `api_tests/folders.rs`: `search_survives_a_quote` |
| `test_resolving_a_duplicate_updates_the_index` | `api_tests/folders.rs`: `resolving_a_duplicate_updates_the_index` |
| `test_untagged_files_still_report_duration` | `api_tests/folders.rs`: `untagged_files_still_report_their_duration` (un mp3 hecho ya sin etiquetas, en vez de quitárselas con mutagen) |
| `test_the_interface_uses_the_query_parameters_the_api_expects` | `api_tests/files.rs`: `the_interface_sends_the_query_parameters_the_api_requires` (recorre `routes/` buscando cada `ctx.query()….required()` y mira que `desktop/src/api.js` lo mande, como `nombre=` o en un `URLSearchParams({…})`) |
| `test_the_interface_sends_the_body_fields_the_api_reads` | `api_tests/files.rs`: `the_interface_sends_the_fields_the_api_reads` |
| `test_rescanning_reuses_files_that_have_not_changed` | `api_tests/files.rs`: `rescanning_reuses_files_that_have_not_changed` (los escaneos, por `/api/scan`) |
| `test_a_lost_database_is_rebuilt_from_the_files` | `api_tests/files.rs`: `a_lost_database_is_rebuilt_from_the_files` (la que destapó lo de la base borrada por fuera) |
| `test_the_cover_cache_notices_when_the_file_changes` | `api_tests/files.rs`: `the_cover_cache_notices_changes_and_keeps_its_budget` |
| `test_the_cover_cache_stays_within_its_memory_budget` | `api_tests/files.rs`: `the_cover_cache_notices_changes_and_keeps_its_budget` (el tope, por `media::cover_cache_bytes`) |
| `test_blurring_a_cover_does_not_touch_the_image` | `api_tests/files.rs`: `blurring_a_cover_does_not_touch_the_image` |
| `test_blur_survives_a_lost_database` | `api_tests/files.rs`: `a_lost_database_is_rebuilt_from_the_files` |
| `test_blurring_an_unknown_song_is_a_404` | `api_tests/files.rs`: `blurring_a_cover_does_not_touch_the_image` |
| `test_the_blur_column_is_added_to_an_older_database` | `api_tests/files.rs`: `the_blur_column_is_added_to_an_older_database` (abriéndola con `library::index::schema::connect` en vez de cambiar `config.DATABASE`) |
| `test_the_duplicate_report_uses_the_keys_the_interface_reads` | `api_tests/files.rs`: `the_duplicate_report_uses_the_keys_the_interface_reads` (lee `services/duplicates.rs` entero; las claves viejas, entre comillas, que en los comentarios sí sale la palabra) |
| `test_with_a_token_nothing_passes_without_it` | `api_tests/guard.rs`: `with_a_token_nothing_passes_without_it` (una app con su `Guard`, de `testkit::guarded`, en vez de cambiar `A._TOKEN`) |
| `test_in_browser_mode_a_plain_post_is_refused` | `api_tests/guard.rs`: `in_browser_mode_a_plain_request_is_refused` |
| `test_the_token_compare_does_not_break_with_odd_bytes` | `api_tests/guard.rs`: `with_a_token_nothing_passes_without_it` («Bearer señor» como texto; en Python iban los bytes en latin-1) |
| `test_the_api_does_not_publish_its_schema` | `api_tests/guard.rs`: `the_api_does_not_publish_its_schema` |
| `test_serving_outside_this_machine_needs_a_token` | `api_tests/guard.rs`: `what_is_loopback` (qué es este equipo. No mira que sin `DANPLAY_TOKEN` no se escuche fuera: lo decide `main.rs` al arrancar, con `exit(1)`) |
| `test_an_unknown_host_is_refused` | `api_tests/guard.rs`: `in_browser_mode_a_plain_request_is_refused` |
| `test_editing_only_accepts_the_fields_a_person_can_fix` | `api_tests/files.rs`: `editing_only_accepts_what_a_person_can_fix` |
| `test_stars_out_of_range_are_refused` | `api_tests/files.rs`: `editing_only_accepts_what_a_person_can_fix` |
| `test_resolving_refuses_paths_outside_the_library` | `api_tests/files.rs`: `resolving_and_covers_refuse_what_is_outside_or_not_an_image` |
| `test_a_cover_has_to_be_an_image` | `api_tests/files.rs`: `resolving_and_covers_refuse_what_is_outside_or_not_an_image` |
| `test_the_audio_says_its_real_format` | `api_tests/files.rs`: `the_audio_says_its_real_format` (`services::media::audio_type` en vez de `api.audio_type`) |
| `test_the_assistant_does_not_delete_on_its_own` | `api_tests/assistant.rs`: `the_assistant_does_not_delete_on_its_own` (que la canción sigue ahí, en vez de un `library.trash` de mentira) |
| `test_confirming_is_only_for_the_three_that_need_it` | `api_tests/assistant.rs`: `confirming_is_only_for_what_needs_it` |
| `test_the_confirm_endpoint_refuses_anything_else` | `api_tests/assistant.rs`: `confirming_is_only_for_what_needs_it` |
| `test_download_button_downloads_and_releases_the_lock` | `api_tests/downloads.rs`: `the_download_button_downloads_and_releases_the_lock` (con un yt-dlp de mentira en `sh` en vez de un `youtube.download` de mentira: los 3 resultados, en la consulta `ytsearch3:`) |
| `test_two_downloads_at_once_are_not_allowed` | `api_tests/downloads.rs`: `two_downloads_at_once_are_not_allowed` |
| `test_confirming_an_assistant_download_starts_with_its_args` | `api_tests/downloads.rs`: `confirming_an_assistant_download_starts_with_its_args` (un resultado, por `ytsearch1:`; el `source`, en el historial de descargas) |
| `test_confirm_with_nothing_to_download_is_a_clear_400` | `api_tests/assistant.rs`: `confirming_is_only_for_what_needs_it` (el 400 y su motivo. No mira que no se baja nada: esa prueba no tiene yt-dlp de mentira) |
| `test_confirm_also_accepts_a_bare_query` | `api_tests/downloads.rs`: `confirming_also_accepts_a_bare_query` |
| `test_the_download_plan_sanitizes_the_arguments` | `api_tests/assistant.rs`: `the_download_plan_sanitizes_and_the_dialog_says_what_and_warns` (`library/src/assistant/tools/tests.rs`: `the_download_plan_cleans_and_dedupes` ya miraba los repetidos y el recorte, con otros valores) |
| `test_confirm_dialog_says_what_will_be_downloaded` | `api_tests/assistant.rs`: `the_download_plan_sanitizes_and_the_dialog_says_what_and_warns` |
| `test_a_made_up_id_enters_no_playlist` | `api_tests/assistant.rs`: `a_made_up_id_enters_no_playlist` |
| `test_creating_a_playlist_returns_what_really_went_in` | `api_tests/assistant.rs`: `creating_a_playlist_returns_what_really_went_in` |
| `test_playlists_are_referred_to_by_their_name` | `api_tests/assistant.rs`: `playlists_are_referred_to_by_their_name` (`library/src/assistant/tools/tests.rs`: `playlists_are_found_by_name_first` ya miraba el nombre por delante del id, con una biblioteca de mentira) |
| `test_fixing_a_playlist_leaves_it_exactly_as_asked` | `api_tests/assistant.rs`: `fixing_removing_and_renaming_a_playlist` |
| `test_remove_and_rename` | `api_tests/assistant.rs`: `fixing_removing_and_renaming_a_playlist` |
| `test_revision_goes_up_with_every_visible_change` | `api_tests/assistant.rs`: `the_revision_goes_up_with_every_visible_change` (los cambios, por la API; la revisión, la de `/api/status`) |
| `test_narration_detector_recognizes_what_happened` | `api_tests/assistant.rs`: `the_narration_detector_recognizes_what_happened` (`library/src/assistant/narration.rs`: `tests::action_claims` ya miraba cuatro frases) |
| `test_a_faked_tool_mark_is_removed_and_counts_as_a_lie` | `api_tests/assistant.rs`: `a_faked_tool_mark_is_removed_and_counts_as_a_lie` (lo de `strip_markers` y `has_markers`, en `the_narration_detector_recognizes_what_happened`) |
| `test_if_no_phrase_triggers_the_judge_decides` | `api_tests/assistant.rs`: `if_no_phrase_gives_it_away_the_judge_decides` |
| `test_yes_to_its_own_question_forces_tool_use` | `api_tests/assistant.rs`: `yes_to_its_own_question_forces_tool_use` |
| `test_history_notes_are_system_messages_not_its_text` | `api_tests/assistant.rs`: `the_history_notes_are_system_messages_not_its_text` |
| `test_claiming_an_action_without_tools_forces_it_to_act` | `api_tests/assistant.rs`: `claiming_an_action_without_tools_forces_it_to_act` |
| `test_stopped_only_once_and_only_if_it_used_nothing` | `api_tests/assistant.rs`: `it_is_stopped_only_once_and_only_if_it_used_nothing` |
| `test_a_normal_reply_is_left_alone` | `api_tests/assistant.rs`: `a_normal_reply_is_left_alone` |
| `test_every_turn_carries_the_real_app_state` | `api_tests/assistant.rs`: `every_turn_carries_the_real_app_state` |
| `test_tool_messages_are_tagged_with_what_they_did` | `api_tests/assistant.rs`: `the_history_notes_are_system_messages_not_its_text` |
| `test_a_failing_tool_does_not_count_as_done` | `api_tests/assistant.rs`: `a_failing_tool_does_not_count_as_done` |
| `test_download_done_notice_does_not_trigger_the_detector` | `api_tests/assistant.rs`: `the_download_done_notice_does_not_trigger_the_detector` |
| `test_second_confirmation_in_a_turn_is_clearly_refused` | `api_tests/assistant.rs`: `a_second_confirmation_in_a_turn_is_clearly_refused` |
| `test_narrating_again_after_the_nudge_is_shown` | `api_tests/assistant.rs`: `it_is_stopped_only_once_and_only_if_it_used_nothing` (la última parte) |
| `test_the_judge_is_not_bothered_by_music_knowledge` | `api_tests/assistant.rs`: `if_no_phrase_gives_it_away_the_judge_decides` |
| `test_after_download_followup_is_forced_and_playlist_is_real` | `api_tests/assistant.rs`: `the_after_download_follow_up_is_forced_and_the_playlist_is_real` |
| `test_yes_to_something_the_app_did_does_not_force_a_repeat` | `api_tests/assistant.rs`: `yes_to_its_own_question_forces_tool_use` |
| `test_real_state_lists_the_latest_downloads_with_their_id` | `api_tests/assistant.rs`: `the_real_state_lists_the_latest_downloads_with_their_id` |
| `test_dialog_warns_if_that_url_was_downloaded_recently` | `api_tests/assistant.rs`: `the_download_plan_sanitizes_and_the_dialog_says_what_and_warns` (`library/src/assistant/tools/tests.rs`: `the_dialog_warns_of_a_recent_download` ya miraba el aviso, con una biblioteca de mentira) |
| `test_the_prompt_leaves_confirmations_to_the_app` | `api_tests/assistant.rs`: `the_prompt_leaves_confirmations_to_the_app` |
| `test_each_tool_result_travels_to_the_next_turn` | `api_tests/assistant.rs`: `each_tool_result_travels_to_the_next_turn` |
| `test_history_window_starts_at_user_and_keeps_the_request` | `api_tests/assistant.rs`: `the_history_window_starts_at_the_user_and_keeps_the_request` (`library/src/assistant/context.rs`: `tests::window_starts_at_a_user_message` ya miraba el principio, el tope y el mensaje enorme, sin los mensajes de la app) |
| `test_brief_summarizes_with_ids_and_names` | `api_tests/assistant.rs`: `brief_summarizes_with_ids_and_names` (`library/src/assistant/context.rs`: `tests::what_each_tool_returned` ya miraba los cuatro primeros, sin el «y 5 mas») |
| `test_pending_download_is_not_summarized_as_zero_downloaded` | `api_tests/assistant.rs`: `a_pending_download_is_not_summarized_as_zero_downloaded` |
| `test_rename_a_playlist_through_the_api` | `api_tests/assistant.rs`: `fixing_removing_and_renaming_a_playlist` (la parte del `PATCH`; `routes/tests.rs`: `playlists_from_start_to_end` ya miraba el 400, el 409 y el 404) |
| `test_an_outside_song_plays_without_entering_the_library` | `api_tests/external.rs`: `an_outside_song_plays_without_entering_the_library` |
| `test_playing_it_again_does_not_duplicate_it_and_moves_it_up` | `api_tests/external.rs`: `playing_it_again_moves_it_up_without_duplicating` |
| `test_a_library_song_opened_from_outside_stays_the_library_one` | `api_tests/external.rs`: `a_library_song_opened_from_outside_stays_the_library_one` |
| `test_an_outside_song_serves_its_audio_and_its_cover_route` | `api_tests/external.rs`: `an_outside_song_serves_its_audio_and_can_be_looked_at_but_not_touched` |
| `test_saving_the_list_keeps_it_after_discarding` | `api_tests/external.rs`: `saving_dropping_and_missing_files` |
| `test_an_empty_list_cannot_be_saved` | `api_tests/external.rs`: `saving_dropping_and_missing_files` (`routes/tests.rs`: `what_is_opened_from_outside` ya lo miraba) |
| `test_one_song_can_be_dropped_from_the_list` | `api_tests/external.rs`: `saving_dropping_and_missing_files` |
| `test_a_file_that_is_not_there_is_not_added` | `api_tests/external.rs`: `saving_dropping_and_missing_files` (`routes/tests.rs`: `what_is_opened_from_outside` ya lo miraba) |
| `test_an_outside_song_can_be_looked_at_but_not_touched` | `api_tests/external.rs`: `an_outside_song_serves_its_audio_and_can_be_looked_at_but_not_touched` |
| `test_chat_messages_have_a_shape` | `api_tests/external.rs`: `chat_messages_and_study_markers_have_a_shape` |
| `test_study_markers_have_a_shape` | `api_tests/external.rs`: `chat_messages_and_study_markers_have_a_shape` |

La IA de mentira es un servidor de verdad en 127.0.0.1 (`testkit::FakeAi`),
con la API de OpenAI, al que el perfil o el borrador apuntan con `base_url`:
lo que en Python era un cliente de mentira (`_ScriptedTurns`, `_with_client`).
En `api_tests/assistant.rs`, `script` la pone como perfil activo, contesta
por turnos y hace de juez. El yt-dlp de mentira (`testkit::fake_ytdlp`) es un
guion de `sh` que apunta cada llamada y deja un mp3 de verdad: por eso las de
`api_tests/downloads.rs` son solo de Unix.

### tests/test_stems.py (35)

| Python | Rust |
| --- | --- |
| `test_a_song_is_separated_into_its_own_folder` | `api_tests/stems.rs`: `a_song_is_separated_into_its_own_folder` |
| `test_the_quick_stems_can_be_used_while_they_are_improved` | `api_tests/stems.rs`: `the_quick_stems_can_be_used_while_they_are_improved` |
| `test_quick_ones_go_before_any_improvement` | `api_tests/stems.rs`: `quick_ones_go_before_any_improvement` (`services/stems.rs`: `tests::quick_ones_go_before_improvements` ya miraba el orden de la cola) |
| `test_what_the_song_does_not_have_is_not_a_track` | `api_tests/stems.rs`: `what_the_song_does_not_have_is_not_a_track` |
| `test_quick_stems_left_halfway_are_only_improved` | `api_tests/stems.rs`: `quick_stems_left_halfway_are_only_improved` (la mejora la hace fallar el motor de mentira, con `fail_refine`, en vez de un `refine_song` de mentira) |
| `test_the_lists_say_whether_the_stems_are_the_best` | `api_tests/stems.rs`: `the_lists_say_which_songs_have_stems_and_whether_they_are_the_best` |
| `test_the_stems_can_be_kept_in_opus` | `api_tests/stems.rs`: `the_stems_can_be_kept_in_opus` (el formato, por `/api/settings`; si ffmpeg no sabe Opus, se salta igual) |
| `test_what_a_crash_left_halfway_is_swept_away` | `api_tests/stems.rs`: `what_a_crash_left_halfway_is_swept_away_and_stems_are_not_songs` |
| `test_the_stems_are_not_songs_of_the_library` | `api_tests/stems.rs`: `what_a_crash_left_halfway_is_swept_away_and_stems_are_not_songs` |
| `test_the_lists_say_which_songs_have_stems` | `api_tests/stems.rs`: `the_lists_say_which_songs_have_stems_and_whether_they_are_the_best` |
| `test_a_lost_index_finds_the_stems_again` | `api_tests/stems.rs`: `a_lost_index_finds_the_stems_again` |
| `test_stems_of_before_1_31_are_still_found` | `api_tests/stems.rs`: `stems_of_before_1_31_are_still_found` |
| `test_stems_deleted_by_hand_are_forgotten` | `api_tests/stems.rs`: `stems_deleted_by_hand_are_forgotten` |
| `test_separating_again_replaces_the_old_stems` | `api_tests/stems.rs`: `separating_again_replaces_the_old_stems` (a la papelera de las pruebas en vez de un `trash_path` de mentira) |
| `test_two_songs_with_the_same_name_do_not_share_a_folder` | `api_tests/stems.rs`: `two_songs_with_the_same_name_do_not_share_a_folder` |
| `test_the_queue_separates_one_after_another` | `api_tests/stems.rs`: `the_queue_separates_one_after_another` |
| `test_a_song_that_fails_does_not_stop_the_queue` | `api_tests/stems.rs`: `a_song_that_fails_does_not_stop_the_queue` |
| `test_cancelling_stops_and_empties_the_queue` | ya estaba en `routes/stems.rs`: `tests::cancelling_stops_the_separator_and_empties_the_queue` (con un `sh` lento. Pide una sola canción: no mira que se vaya de la cola otra que esperaba) |
| `test_the_weights_are_checked_before_being_used` | `api_tests/stems.rs`: `the_weights_are_checked_before_being_used` (un servidor en 127.0.0.1 en vez de un `urlopen` de mentira) |
| `test_the_separator_is_one_download_with_its_specialists` | `api_tests/stems.rs`: `the_separator_is_one_download_with_its_specialists` |
| `test_trashing_a_song_takes_its_stems_along` | `api_tests/stems.rs`: `trashing_a_song_takes_its_stems_along` (a la papelera de las pruebas) |
| `test_the_mixer_is_saved_with_the_study` | `api_tests/stems.rs`: `the_mixer_is_saved_with_the_study` |
| `test_the_mix_filter_pans_and_mutes_like_the_player` | `api_tests/stems.rs`: `the_mix_filter_pans_mutes_and_goes_by_sides_like_the_engine` |
| `test_the_mix_without_drums_is_saved_and_joins_the_library` | `api_tests/stems.rs`: `the_mix_without_drums_is_saved_and_joins_the_library` (la duración, con `library::tags::read` en vez de mutagen) |
| `test_the_mix_can_carry_the_click` | `api_tests/stems.rs`: `the_mix_can_carry_the_click` (lo del filtro, en `the_mix_filter_pans_mutes_and_goes_by_sides_like_the_engine`) |
| `test_a_mix_cannot_go_outside_the_library_nor_over_a_song` | `api_tests/stems.rs`: `a_mix_cannot_go_outside_the_library_nor_over_a_song` |
| `test_a_song_goes_to_the_phone_in_one_file` | `api_tests/stems.rs`: `a_song_goes_to_the_phone_in_one_file` (la rejilla, guardada antes con `beats::save_exact` en vez de un `grid_of` de mentira; Vorbis, 44,1 kHz y estéreo, según `ffmpeg -i`. No mira `version`, y de `waves.tracks` solo cuántas hay) |
| `test_the_spoken_guides_travel_with_the_song` | `api_tests/stems.rs`: `the_spoken_guides_travel_with_the_song` (que la guía está en el ZIP; no mira que empiece por `RIFF`) |
| `test_taking_it_again_replaces_the_old_one` | `api_tests/stems.rs`: `taking_it_again_replaces_the_old_one_and_without_vorbis_it_goes_in_mp3` (con rejilla. No mira la canción sin pulso: `click` falso, su `click_error` y `grid` nulo) |
| `test_without_vorbis_the_parts_go_in_mp3` | `api_tests/stems.rs`: `taking_it_again_replaces_the_old_one_and_without_vorbis_it_goes_in_mp3` (lo que dice ffmpeg de sus códecs, con `stems::set_encoders` en vez de un `_encoders` de mentira) |
| `test_a_whole_set_goes_and_the_ones_without_parts_say_why` | `api_tests/stems.rs`: `a_whole_set_goes_and_the_ones_without_parts_say_why` |
| `test_the_api_separates_lists_and_deletes_stems` | `api_tests/stems.rs`: `the_api_separates_lists_and_deletes_stems` |
| `test_the_mix_carries_the_guides_or_not_also_the_song_alone` | `api_tests/stems.rs`: `the_mix_carries_the_guides_or_not_also_the_song_alone` (lo del filtro, en `the_mix_filter_pans_mutes_and_goes_by_sides_like_the_engine`) |
| `test_each_track_goes_by_its_side_or_in_mono_like_the_engine` | `api_tests/stems.rs`: `the_mix_filter_pans_mutes_and_goes_by_sides_like_the_engine` |
| `test_the_mix_by_sides_sounds_like_a_live_sequence` | `api_tests/stems.rs`: `the_mix_by_sides_sounds_like_a_live_sequence` (no mira que la derecha calle después de la guía, ni el 422 de un lado «C» por la API) |

El `FAKE_ENGINE` de Python es ahora un motor de mentira en Rust y en el
mismo proceso (`engine`, en `api_tests/stems.rs`, enganchado con
`Separator::set_engine`): tonos de un segundo con ffmpeg de verdad, con los
mismos `.esperar` y `.callar`, y `fail_refine` para que falle la mejora. El
audio se mide decodificándolo con ffmpeg, sin numpy.

### tests/test_plan.py (48)

| Python | Rust |
| --- | --- |
| `test_the_server_sets_id_summary_and_effect` | `api_tests/plan.rs`: `the_server_sets_id_summary_and_effect` |
| `test_model_args_cannot_sneak_in_after_or_plan` | `api_tests/plan.rs`: `model_args_cannot_sneak_in_an_after_or_a_plan` |
| `test_it_expires_with_the_clock` | `api_tests/plan.rs`: `it_expires_with_the_clock` (el reloj a mano, `plan::testing::CLOCK`, lo que era `plan._now`) |
| `test_a_new_one_replaces_the_previous_of_the_same_tool` | `api_tests/plan.rs`: `a_new_one_replaces_the_previous_of_the_same_tool` |
| `test_entry_cap_drops_the_oldest` | `api_tests/plan.rs`: `the_entry_cap_drops_the_oldest` |
| `test_concurrent_claims_only_one_wins` | `api_tests/plan.rs`: `concurrent_claims_only_one_wins` |
| `test_selection_filters_and_empty_does_not_spend` | `api_tests/plan.rs`: `the_selection_filters_and_empty_does_not_spend` |
| `test_delete_by_id_checks_it_is_still_the_same` | `api_tests/plan.rs`: `deleting_by_id_checks_it_is_still_the_same` |
| `test_an_after_that_does_not_fit_is_not_registered` | `api_tests/plan.rs`: `an_after_that_does_not_fit_is_not_registered` (los 13 «después» hostiles) |
| `test_apply_after_with_too_many_slots_writes_nothing` | `api_tests/plan.rs`: `apply_after_with_too_many_slots_writes_nothing` (no se pasa lo de que no se escriben etiquetas: el espía `stamps`) |
| `test_route_410_when_no_longer_live` | `api_tests/plan.rs`: `the_route_says_410_when_it_is_no_longer_live` (con un yt-dlp de mentira en `sh` en vez de un `youtube.download` de mentira) |
| `test_route_409_without_spending_the_proposal` | `api_tests/plan.rs`: `the_route_409_does_not_spend_the_proposal` (el «un resultado, del asistente», por la consulta `ytsearch1:` y el historial) |
| `test_route_503_and_no_folder_do_not_spend_either` | `api_tests/plan.rs`: `without_a_folder_the_route_says_choose_and_does_not_spend` (el 503 no se pasa: el núcleo busca yt-dlp en `tools/` y en el PATH, y si el equipo tiene uno no hay cómo quitarlo; `routes/assistant.rs`: `tests::confirmations` ya admitía 503 o 409) |
| `test_route_nothing_checked_is_400_and_releases_the_turn` | `api_tests/plan.rs`: `nothing_checked_is_400_and_releases_the_turn` |
| `test_the_client_cannot_sneak_args_into_the_new_form` | `api_tests/plan.rs`: `the_client_cannot_sneak_args_into_the_new_form` |
| `test_the_old_form_still_works` | `api_tests/plan.rs`: `the_old_form_still_works_and_an_after_from_the_client_does_nothing` |
| `test_an_after_from_the_client_in_the_old_form_does_nothing` | `api_tests/plan.rs`: `the_old_form_still_works_and_an_after_from_the_client_does_nothing` |
| `test_delete_by_id_uses_the_registered_args` | `api_tests/plan.rs`: `deleting_by_id_uses_the_registered_args` (lo de `delete_song` con un argumento de más, en `deleting_by_id_checks_it_is_still_the_same`: mira lo que se gasta en vez de un `chat.confirm` espía) |
| `test_status_and_plan_routes` | `api_tests/plan.rs`: `the_status_and_plan_routes` |
| `test_accepting_downloads_the_checked_and_fills_the_playlist` | `api_tests/plan.rs`: `accepting_downloads_the_checked_ones_and_fills_the_playlist` (las canciones nuevas son las que deja el yt-dlp de mentira) |
| `test_never_more_than_two_lookups_at_once` | `api_tests/plan.rs`: `never_more_than_two_lookups_at_once` |
| `test_cuts_the_batch_at_the_first_sign_of_blocking` (3 casos) | `api_tests/plan.rs`: `the_batch_is_cut_at_the_first_sign_of_blocking` |
| `test_if_the_first_block_leaves_nothing_it_is_an_error` | `api_tests/plan.rs`: `if_the_first_block_leaves_nothing_it_is_an_error_and_without_yt_dlp_too` |
| `test_without_yt_dlp_the_plan_errors_and_does_not_look_up` | `api_tests/plan.rs`: `if_the_first_block_leaves_nothing_it_is_an_error_and_without_yt_dlp_too` (lo que decía `_warm`, con `plan::testing::WARM`) |
| `test_lookups_are_remembered_for_ten_minutes` | `api_tests/plan.rs`: `lookups_are_remembered_for_ten_minutes_but_not_a_block` |
| `test_a_blocking_failure_is_not_remembered` | `api_tests/plan.rs`: `lookups_are_remembered_for_ten_minutes_but_not_a_block` |
| `test_at_most_25_rows_per_card` | `api_tests/plan.rs`: `at_most_25_rows_per_card` |
| `test_hostile_links_are_rejected_and_not_looked_up` | `api_tests/plan.rs`: `hostile_links_are_rejected_and_not_looked_up` |
| `test_only_hostile_links_is_not_registered` | `api_tests/plan.rs`: `only_hostile_links_is_not_registered` |
| `test_an_odd_duration_comes_out_unchecked` (2 casos) | `api_tests/plan.rs`: `an_odd_duration_comes_out_unchecked` |
| `test_a_dissimilar_title_comes_out_unchecked` | `api_tests/plan.rs`: `a_dissimilar_title_comes_out_unchecked_and_what_is_not_found_too` |
| `test_what_is_not_found_comes_out_unchecked` | `api_tests/plan.rs`: `a_dissimilar_title_comes_out_unchecked_and_what_is_not_found_too` |
| `test_titles_and_channels_go_through_datum` | `api_tests/plan.rs`: `titles_and_channels_go_through_datum_and_odd_ids_are_not_shown` |
| `test_a_video_without_a_valid_id_is_not_shown_or_downloaded` | `api_tests/plan.rs`: `titles_and_channels_go_through_datum_and_odd_ids_are_not_shown` |
| `test_what_you_already_have_is_flagged` | `api_tests/plan.rs`: `what_you_already_have_is_flagged` (lo que ya tienes, con `plan::testing::MINE`) |
| `test_what_the_registrant_says_wins_in_the_checkbox` | `api_tests/plan.rs`: `what_the_registrant_says_wins_in_the_checkbox` |
| `test_stopping_the_search_does_not_spend_the_proposal` | `api_tests/plan.rs`: `stopping_the_search_does_not_spend_the_proposal` |
| `test_accepting_while_searching_uses_what_is_resolved` | `api_tests/plan.rs`: `accepting_while_searching_uses_what_is_resolved` |
| `test_each_download_goes_after_its_previous_slot` | `api_tests/plan.rs`: `each_download_goes_after_its_previous_slot` (los contadores de los espías no se pasan; que es una sola escritura ya lo contaba `library/src/assistant/plan/tests.rs`: `each_download_goes_after_its_previous_slot`, con su base de mentira) |
| `test_without_previous_goes_before_next_else_at_the_end` | `api_tests/plan.rs`: `without_a_previous_one_it_goes_before_the_next_else_at_the_end` |
| `test_ones_inserted_in_the_same_pass_also_anchor` | `api_tests/plan.rs`: `without_a_previous_one_it_goes_before_the_next_else_at_the_end` |
| `test_is_idempotent_and_does_not_duplicate` | `api_tests/plan.rs`: `it_is_idempotent_and_does_not_duplicate` (no se pasa que la segunda vez no escribe, el espía `spy_place`; ya lo contaba `library/src/assistant/plan/tests.rs`: `each_download_goes_after_its_previous_slot`) |
| `test_what_you_already_had_goes_in_as_its_song` | `api_tests/plan.rs`: `it_is_idempotent_and_does_not_duplicate` |
| `test_a_deleted_or_renamed_playlist_is_not_written` | `api_tests/plan.rs`: `a_deleted_or_renamed_playlist_is_not_written` |
| `test_a_song_deleted_by_hand_is_counted` | `api_tests/plan.rs`: `a_song_deleted_by_hand_is_counted_and_failures_say_why` |
| `test_canceled_halfway_and_failures_with_their_reason` | `api_tests/plan.rs`: `a_song_deleted_by_hand_is_counted_and_failures_say_why` |
| `test_unchecked_boxes_are_skipped` | `api_tests/plan.rs`: `unchecked_boxes_are_skipped` |
| `test_real_run_many_publishes_after_before_releasing_the_turn` | ya estaba en `services/youtube/tests.rs`: `run_many_runs_the_finish_before_releasing` (el remate con el turno cogido y su token en `after`; que `after` está antes de soltar el turno y la lista entera al soltarlo, en `after_is_published_before_releasing_the_turn` y `when_active_turns_false_the_playlist_is_complete`, de test_youtube_after.py) |

El registro de propuestas es uno para todo el proceso: estas pruebas van una
detrás de otra con `testkit::registry()` (el mismo cerrojo que cogen las del
chat y toda IA de mentira; reentrante, cuenta por hilo), y cada una empieza
y acaba con `plan::testing::clear()`. En `plan::testing`: `CLOCK`, el reloj
(`plan._now`); `INFO`, la consulta a YouTube (`youtube.info`); `WARM`, lo que
se mira antes de buscar (`_warm`); `MINE`, si ya la tienes
(`youtube.already_in_library`); `SEEN`, lo que llega a `register` tal cual
(lo usa `chat_flow.rs`). Sin `INFO`, el plan no busca: contesta `OFFLINE`.

### tests/test_chat_flow.py (28)

| Python | Rust |
| --- | --- |
| `test_playlist_name_is_understood_as_people_say_it` | `api_tests/chat_flow.rs`: `the_playlist_name_is_understood_as_people_say_it` |
| `test_pasted_setlist_goes_in_order_and_missing_is_offered` | `api_tests/chat_flow.rs`: `a_pasted_setlist_goes_in_order_and_what_is_missing_is_offered` (lo que pide permiso, en lo que recibió el registro de verdad, `plan::testing::SEEN`; la tarjeta queda viva en vez de llamarse `conf1`) |
| `test_check_changes_nothing` | `api_tests/chat_flow.rs`: `check_changes_nothing` |
| `test_set_exactly_only_when_the_user_says_so` | `api_tests/chat_flow.rs`: `set_exactly_only_when_the_user_says_so` |
| `test_a_new_playlist_only_if_the_user_asked_for_it` | `api_tests/chat_flow.rs`: `a_new_playlist_only_if_the_user_asked_for_it` |
| `test_no_download_if_the_user_said_so` | `api_tests/chat_flow.rs`: `no_download_if_the_user_said_so` |
| `test_an_old_setlist_does_not_override_the_new_request` | `api_tests/chat_flow.rs`: `an_old_setlist_does_not_override_the_new_request` |
| `test_model_cannot_sneak_items_into_a_pasted_setlist` | `api_tests/chat_flow.rs`: `the_model_cannot_sneak_items_into_a_pasted_setlist` |
| `test_another_version_chosen_by_number` | `api_tests/chat_flow.rs`: `another_version_chosen_by_number` |
| `test_find_songs_only_looks` | `api_tests/chat_flow.rs`: `find_songs_only_looks` |
| `test_downloading_what_we_have_asks_no_permission` | `api_tests/chat_flow.rs`: `downloading_what_we_have_asks_no_permission` |
| `test_download_only_what_is_missing` | `api_tests/chat_flow.rs`: `download_only_what_is_missing` |
| `test_urls_are_not_lowercased_or_repeated` | `api_tests/chat_flow.rs`: `urls_are_not_lowercased_or_repeated` |
| `test_search_songs_retries_without_requiring_all_words` | `api_tests/chat_flow.rs`: `search_songs_retries_without_requiring_all_words` (la búsqueda de siempre, por `/api/search`) |
| `test_search_youtube_normalizes_text_but_not_links` | `api_tests/chat_flow.rs`: `search_youtube_normalizes_text_but_not_links` (lo que se busca, en lo que apuntó un yt-dlp de mentira en vez de un `youtube.info` de mentira) |
| `test_a_pasted_setlist_costs_one_model_call` | `api_tests/chat_flow.rs`: `a_pasted_setlist_costs_one_model_call` |
| `test_model_view_of_a_setlist_omits_the_full_table` | `api_tests/chat_flow.rs`: `the_model_view_of_a_setlist_omits_the_full_table` |
| `test_a_call_written_as_text_does_not_set_the_playlist` | `api_tests/chat_flow.rs`: `a_call_written_as_text_does_not_set_the_playlist` |
| `test_the_turn_has_a_call_cap` | `api_tests/chat_flow.rs`: `the_turn_has_a_call_cap` (por la API manda el tope de vueltas: corta a las 6, con «Me he enredado con las consultas». El de llamadas, con «No he podido terminar», ya estaba en `library/src/assistant/conversation/tests.rs`: `too_many_calls_are_refused_and_the_budget_runs_out`) |
| `test_a_huge_message_reaches_the_model_trimmed` | `api_tests/chat_flow.rs`: `a_huge_message_reaches_the_model_trimmed` |
| `test_what_was_asked_before_pasting_the_setlist_counts` | `api_tests/chat_flow.rs`: `what_was_asked_before_pasting_the_setlist_counts` |
| `test_an_old_download_does_not_give_another_songs_id` | `api_tests/chat_flow.rs`: `an_old_download_does_not_give_another_songs_id` (`chat::still_there` dice sí o no en vez de devolver la canción) |
| `test_the_prompt_fits_and_keeps_the_rules` | `api_tests/chat_flow.rs`: `the_prompt_fits_and_keeps_the_rules` |
| `test_intake_and_setlist_invent_no_setlist_in_small_talk` | `api_tests/chat_flow.rs`: `small_talk_invents_no_setlist` |
| `test_the_real_card_accepts_what_the_setlist_proposes` | `api_tests/chat_flow.rs`: `the_real_card_accepts_what_the_setlist_proposes` (la búsqueda en YouTube, de mentira con `plan::testing::INFO`; con `WARM` no se busca yt-dlp, y espera 10 s en vez de 90) |
| `test_among_versions_the_one_already_in_a_playlist_wins` | `api_tests/chat_flow.rs`: `among_versions_the_one_already_in_a_playlist_wins` |
| `test_empty_reply_after_tools_says_what_was_done` | `api_tests/chat_flow.rs`: `an_empty_reply_after_tools_says_what_was_done` |
| `test_the_request_is_not_reported_as_an_ignored_line` | `api_tests/chat_flow.rs`: `the_request_is_not_reported_as_an_ignored_line` |

El modelo de mentira es el `FakeAi` con su guion (`api_tests/assistant.rs`:
`script`), y se le habla por `/api/chat`. Lo que en Python era un
`plan.register` de mentira (el fixture `asked`) es el registro de verdad, y
`plan::testing::SEEN` apunta lo que recibió.

### tests/test_downloads.py (13)

| Python | Rust |
| --- | --- |
| `test_a_download_is_filed_and_indexed` | `api_tests/youtube.rs`: `a_download_is_filed_and_indexed` (los topes, en la línea de órdenes de yt-dlp en vez de en sus opciones) |
| `test_youtube_music_wins_and_writes_tags` | `api_tests/youtube.rs`: `youtube_music_wins_and_writes_tags` |
| `test_what_you_already_have_is_not_downloaded_unless_forced` | `api_tests/youtube.rs`: `what_you_already_have_is_not_downloaded_unless_forced` |
| `test_a_drum_cam_is_filed_by_its_artist_and_suggests_tracks` | `api_tests/youtube.rs`: `a_drum_cam_is_filed_by_its_artist_and_suggests_tracks` |
| `test_moving_it_to_tracks_keeps_the_same_song` | `api_tests/youtube.rs`: `moving_it_to_tracks_keeps_the_same_song` (las estrellas y la lista, por la API; `routes/songs/tests.rs`: `tidy_and_move_keep_the_same_song` ya miraba mover a Pistas y el 422) |
| `test_without_filing_it_stays_in_the_inbox` | `api_tests/youtube.rs`: `without_filing_it_stays_in_the_inbox` |
| `test_a_search_brings_several_and_cancel_stops_it` | `api_tests/youtube.rs`: `a_search_brings_several_and_cancel_stops_it` |
| `test_failures_are_counted_and_the_turn_is_released` | `api_tests/youtube.rs`: `failures_are_counted_and_the_turn_is_released` |
| `test_a_non_youtube_link_is_not_passed_to_yt_dlp` | `api_tests/youtube.rs`: `a_non_youtube_link_is_not_passed_to_yt_dlp` (y además mira que esa dirección no llega a yt-dlp) |
| `test_a_long_live_stream_is_rejected` | `api_tests/youtube.rs`: `a_long_live_stream_is_rejected` (`library/src/youtube/tests.rs`: `long_videos_are_filtered_like_python` ya lo miraba) |
| `test_no_chosen_folder_blocks_downloads_and_suggests_one` | `api_tests/youtube.rs`: `no_chosen_folder_blocks_downloads_and_suggests_one` (y además mira que yt-dlp no llega a bajar nada) |
| `test_a_missing_or_outside_folder_is_asked_for_again` | `api_tests/youtube.rs`: `a_missing_or_outside_folder_is_asked_for_again` |
| `test_choosing_a_folder_uses_and_creates_its_subfolders` | `api_tests/youtube.rs`: `choosing_a_folder_uses_and_creates_its_subfolders` (en Python solo corría contra su núcleo; `services/youtube/tests.rs`: `the_folder_must_be_one_of_yours` ya miraba las subcarpetas, el .env y las que no valen, sin la API ni la descarga) |

El yt-dlp de mentira de `api_tests/youtube.rs` es un `sh` (por eso el archivo
es solo de Unix): apunta cada llamada con sus argumentos, contesta con los
vídeos que conoce, una búsqueda los trae todos, y al bajar uno copia un mp3
sintético y avisa del avance. Lo que no conoce es «Video unavailable». Por
eso lo que Python miraba en las opciones de `YoutubeDL` se mira aquí en su
línea de órdenes (`--socket-timeout`, `--no-playlist`, `--yes-playlist`…).

### tests/test_youtube_after.py (41)

| Python | Rust |
| --- | --- |
| `test_on_done_runs_before_releasing_the_turn` | `services/youtube/tests.rs`: `on_done_runs_before_releasing_the_turn` (`run_many_runs_the_finish_before_releasing` ya miraba el turno cogido y la fase «listing», sin consultas) |
| `test_after_is_published_before_releasing_the_turn` | `services/youtube/tests.rs`: `after_is_published_before_releasing_the_turn` |
| `test_without_on_done_after_is_none_and_not_inherited` | `services/youtube/tests.rs`: `without_on_done_after_is_none_and_not_inherited` |
| `test_on_done_returning_none_leaves_after_as_none` | `services/youtube/tests.rs`: `on_done_returning_none_leaves_after_as_none` |
| `test_on_done_raising_is_a_short_error_download_still_done` | `services/youtube/tests.rs`: `on_done_failing_is_a_short_error_and_the_download_is_still_done` (un `Err` en vez de una excepción) |
| `test_an_exception_without_text_says_at_least_its_name` | no se pasa: en Rust el remate falla con un `Err(String)`, sin nombre de excepción. Uno vacío sale «Exception», y eso ya lo miraba `services/youtube/tests.rs`: `what_the_finish_returns_becomes_after` |
| `test_on_done_returning_a_non_dict_is_an_error` (4 casos) | `services/youtube/tests.rs`: `on_done_returning_a_non_dict_is_an_error` |
| `test_after_is_a_copy_of_plain_data` | no se pasa: en Rust `after` es un `Value` de JSON por construcción. No hay tuplas, rutas ni claves que no sean texto que convertir, y quien lo hizo ya no lo puede cambiar |
| `test_an_after_that_cannot_be_published_is_an_error` (3 casos) | no se pasa: una clave que no es texto, un diccionario que se contiene a sí mismo o un NaN no caben en un `Value` de JSON |
| `test_on_done_receives_a_copy_of_the_results` | `services/youtube/tests.rs`: `on_done_receives_a_copy_of_the_results` |
| `test_cancelling_does_not_skip_the_finish` | `services/youtube/tests.rs`: `cancelling_does_not_skip_the_finish` |
| `test_if_a_download_crashes_the_finish_runs_with_what_exists` | `services/youtube/tests.rs`: `if_a_download_fails_hard_the_finish_runs_with_what_exists` (la descarga devuelve un `Err` en vez de lanzar) |
| `test_with_no_queries_the_finish_still_runs_with_nothing` | `services/youtube/tests.rs`: `with_no_queries_the_finish_still_runs_with_nothing` |
| `test_if_another_is_running_its_state_is_left_alone` | `services/youtube/tests.rs`: `if_another_is_running_its_state_is_left_alone` |
| `test_when_active_turns_false_the_playlist_is_complete` | `services/youtube/tests.rs`: `when_active_turns_false_the_playlist_is_complete` |
| `test_claim_gives_a_rising_token_that_is_state_job` | ya estaba en `services/youtube/tests.rs`: `the_turn_gives_a_rising_token_and_only_its_owner_writes` (que `job` sea el token, y que no cambie si está ocupado, en `a_new_turn_does_not_inherit_the_previous_after` y `concurrent_claims_only_one_wins`) |
| `test_concurrent_claims_only_one_wins` | `services/youtube/tests.rs`: `concurrent_claims_only_one_wins` |
| `test_a_new_turn_does_not_inherit_the_previous_after` | `services/youtube/tests.rs`: `a_new_turn_does_not_inherit_the_previous_after` |
| `test_the_old_download_does_not_overwrite_the_new_one` | `services/youtube/tests.rs`: `the_old_download_does_not_overwrite_the_new_one` |
| `test_with_claimed_the_turn_token_is_used_and_job_pins_it` | `services/youtube/tests.rs`: `with_claimed_the_turn_token_is_used_and_job_pins_it` |
| `test_release_with_a_token_only_releases_its_own_turn` | `services/youtube/tests.rs`: `release_with_a_token_only_releases_its_own_turn` |
| `test_progress_only_touches_the_progress_fields` | `services/youtube/tests.rs`: `progress_only_touches_the_progress_fields` |
| `test_the_api_shows_after_and_job` | `services/youtube/tests.rs`: `the_api_shows_after_and_job` (en Python solo corría contra su núcleo) |
| `test_every_run_many_result_carries_what_was_requested` | `services/youtube/tests.rs`: `every_run_many_result_carries_what_was_requested` |
| `test_info_with_timeout_is_a_light_query` | `api_tests/youtube.rs`: `info_with_timeout_is_a_light_query` (las opciones, en la línea de órdenes de yt-dlp. Que el timeout vaya por su nombre no se pasa: en Rust es un argumento más) |
| `test_info_says_why_it_failed` (6 casos) | `api_tests/youtube.rs`: `info_says_why_it_failed` |
| `test_info_cleans_up_the_reason` | `api_tests/youtube.rs`: `info_cleans_up_the_reason` (la que destapó lo del «ERROR:» con color) |
| `test_info_reports_the_last_error_and_only_block_warnings` | `api_tests/youtube.rs`: `info_reports_the_last_error_and_only_block_warnings` (los dos errores, en dos líneas del stderr de un mismo yt-dlp) |
| `test_info_on_an_exception_gives_its_text_or_its_name` | ya estaba en `library/src/youtube/tests.rs`: `the_logger_remembers_why` (sin texto ni nada dicho por yt-dlp, el nombre. En Rust yt-dlp es un ejecutable y no lanza excepciones) |
| `test_info_ok_does_not_change` | `api_tests/youtube.rs`: `info_ok_does_not_change` |
| `test_failure_kind_of_a_non_block_is_empty` (5 casos) | `api_tests/youtube.rs`: `the_failure_kind_of_a_non_block_is_empty` (4 de los 5: `None` no cabe, el motivo es un `&str`) |
| `test_a_foreign_host_never_reaches_yt_dlp` (18 enlaces) | `api_tests/youtube.rs`: `a_foreign_host_never_reaches_yt_dlp` |
| `test_host_of_still_gives_the_domain_of_normal_urls` (10 casos) | `api_tests/youtube.rs`: `host_of_still_gives_the_domain_of_normal_urls` |
| `test_what_is_not_a_url_is_only_searched_on_youtube` (7 textos) | `api_tests/youtube.rs`: `what_is_not_a_url_is_only_searched_on_youtube` |
| `test_info_never_queries_a_non_youtube_host` | `api_tests/youtube.rs`: `info_never_queries_a_non_youtube_host` |
| `test_a_video_with_list_is_queried_as_a_video` (10 enlaces) | `api_tests/youtube.rs`: `a_video_with_list_is_queried_as_a_video` |
| `test_a_deliberate_playlist_is_queried_as_a_playlist` (4 enlaces) | `api_tests/youtube.rs`: `a_deliberate_playlist_is_queried_as_a_playlist` |
| `test_log_records_the_song_you_already_had` | ya estaba en `library/src/youtube/tests.rs`: `the_history_row_points_at_the_song_you_had` (el «ya la tienes», con coincidencia y sin ella; de punta a punta, en `api_tests/youtube.rs`: `what_you_already_have_is_logged_with_its_song`) |
| `test_what_you_already_have_is_logged_with_its_song` | `api_tests/youtube.rs`: `what_you_already_have_is_logged_with_its_song` |
| `test_failed_or_cancelled_results_say_what_was_requested` | `api_tests/youtube.rs`: `failed_or_cancelled_results_say_what_was_requested` |
| `test_download_and_fill_the_playlist_in_the_same_turn` | `api_tests/youtube.rs`: `download_and_fill_the_playlist_in_the_same_turn` |

`youtube::testing` (en `services/youtube.rs`) es uno por app y guarda dos
cosas: la descarga de mentira que usa `run_many` en vez de la de verdad (lo
que en Python era `monkeypatch(youtube, "download")`) y lo que había en
`{active, after}` justo al soltar cada turno (el espía `releases`).

### tests/test_ytdlp.py (17)

| Python | Rust |
| --- | --- |
| `test_the_preferred_runtime_is_the_first_that_works` | `services/ytdlp/tests.rs`: `the_preferred_runtime_is_the_first_that_works` (las opciones, en `--no-js-runtimes --js-runtimes deno:…`) |
| `test_an_old_runtime_is_rejected_with_a_hint` | ya estaba en `library/src/ytdlp/tests.rs`: `each_runtime_says_its_version_its_way` y `the_hint_says_what_to_do` (la versión que dice y la pista, sin programas) |
| `test_with_none_it_says_what_to_install` | ya estaba en `library/src/ytdlp/tests.rs`: `the_hint_says_what_to_do` |
| `test_quickjs_is_called_qjs_and_probed_with_help` (3 versiones) | ya estaba en `library/src/ytdlp/tests.rs`: `each_runtime_says_its_version_its_way` (lo que dice cada una; que se llame `qjs` y se le pregunte con `--help` es su `RuntimeSpec`) |
| `test_the_options_reach_yt_dlp` | ya estaba en `services/ytdlp/tests.rs`: `the_python_options_as_arguments` (`--js-runtimes node:/usr/bin/node`) |
| `test_version_as_numbers` | ya estaba en `library/src/ytdlp/tests.rs`: `versions_compare_as_numbers` |
| `test_it_updates_from_pypi_and_uses_the_new_one` | no se pasa: la rueda de PyPI y cargar el módulo de la carpeta descargada. En Rust yt-dlp es un ejecutable que se pone al día desde GitHub: ya estaba en `services/ytdlp/tests.rs`: `update_from_github_checks_the_sum_and_switches` |
| `test_on_startup_the_downloaded_one_is_loaded` | no se pasa: cargar el módulo descargado. Que al arrancar se usa el descargado ya estaba en `services/ytdlp/tests.rs`: `the_downloaded_one_wins_if_newer_and_answers` |
| `test_a_wheel_not_matching_pypi_is_not_used` | no se pasa: la rueda de PyPI. El sha256 que no coincide con lo que publica GitHub ya estaba en `services/ytdlp/tests.rs`: `update_refuses_what_does_not_match` |
| `test_if_the_new_one_will_not_load_the_bundled_one_is_used` | no se pasa: un módulo que no carga. Uno que dice otra versión ya estaba en `services/ytdlp/tests.rs`: `update_refuses_what_does_not_match` |
| `test_a_broken_download_is_set_aside_on_startup` | no se pasa: un módulo que no carga. Una descargada que dice otra versión, apartada con su marca, ya estaba en `services/ytdlp/tests.rs`: `the_downloaded_one_wins_if_newer_and_answers` |
| `test_a_download_older_than_the_bundled_one_is_not_used` | no se pasa: la carpeta de un módulo. La más vieja que la de la app ya estaba en `services/ytdlp/tests.rs`: `the_downloaded_one_wins_if_newer_and_answers` |
| `test_a_wheel_with_odd_paths_is_not_extracted` | no se pasa: de GitHub baja un ejecutable, no hay rueda que descomprimir |
| `test_what_pypi_lacks_is_reported_in_spanish` | no se pasa: PyPI. El 404 de GitHub, en español, ya estaba en `services/ytdlp/tests.rs`: `update_refuses_what_does_not_match` |
| `test_without_network_it_says_so_in_spanish` | no se pasa: PyPI. GitHub sin red ya estaba en `services/ytdlp/tests.rs`: `update_refuses_what_does_not_match` |
| `test_the_api_reports_the_version_and_updates_as_a_job` | no se pasa: consultaba un PyPI de mentira (en Python ya era solo de su núcleo). Lo de GitHub ya estaba en `services/ytdlp/tests.rs`: `update_from_github_checks_the_sum_and_switches`, por `update`. No mira `/api/youtube` con la versión y el motor de JS, ni la puesta al día como trabajo |
| `test_the_finder_only_recognizes_its_folder` | no se pasa: el buscador de imports de Python (`_Prefer`); un ejecutable no se importa |

### tests/test_serve.py (7)

| Python | Rust |
| --- | --- |
| `test_without_the_variable_the_parent_is_watched_as_usual` | `main.rs`: `tests::the_parent_is_watched_only_with_a_valid_pid` (sin la variable, el núcleo de Rust no vigila nada; el de Python, en Linux y macOS, miraba si cambiaba el padre) |
| `test_with_the_variable_that_process_is_watched` | `main.rs`: `tests::the_parent_is_watched_only_with_a_valid_pid` (uno vivo) y `tests::a_finished_process_is_not_alive` (uno que ya terminó) |
| `test_an_odd_pid_watches_nothing` (4 valores) | `main.rs`: `tests::the_parent_is_watched_only_with_a_valid_pid` |
| `test_on_posix_it_asks_with_signal_zero` | no se pasa: un `os.kill` espía. Rust pregunta con `rustix::process::test_kill_process`, que solo pregunta, y no hay por dónde cambiarlo |
| `test_the_core_starts_on_the_socket_and_answers` | `server/tests/serve.rs`: `the_core_starts_on_the_socket_and_answers` (`danplay-server --uds` con el entorno vacío salvo sus carpetas; se para con SIGTERM) |
| `test_on_startup_and_on_shutdown` | no se pasa: el `lifespan` de FastAPI (en Python ya era solo de su núcleo) |
| `test_serving_writes_a_rotating_log` | no se pasa: el núcleo de Rust escribe sus avisos a stderr y la app los guarda en su registro |

### tests/test_chat_openai.py (5)

| Python | Rust |
| --- | --- |
| `test_no_credit_fallback_answers_and_primary_is_not_retried` | `api_tests/chat_openai.rs`: `without_credit_the_fallback_answers_and_the_primary_is_not_retried` (el estado, por `/api/chat/tools`) |
| `test_a_429_is_retried_then_goes_to_the_fallback` | `api_tests/chat_openai.rs`: `a_429_goes_to_the_fallback_saying_why` (Rust no reintenta como el SDK de openai: mira que contesta el respaldo con el motivo «rate», y que al principal le llegan de 1 a 3 peticiones) |
| `test_a_pasted_setlist_is_a_single_model_request` (entera y en trozos) | `api_tests/chat_openai.rs`: `a_pasted_setlist_is_a_single_model_request` (las dos, por `/api/chat/start` y su sondeo. La IA de mentira manda la respuesta en un trozo: trocearla ya estaba en `library/src/ai/call/tests.rs`: `streamed_with_preamble_and_tool`) |
| `test_two_tools_in_one_message_take_one_more_round` | `api_tests/chat_openai.rs`: `two_tools_in_one_message_take_one_more_round` |
| `test_empty_content_after_tools_says_what_was_done` | ya estaba en `api_tests/chat_flow.rs`: `an_empty_reply_after_tools_says_what_was_done` |

Los dos servidores son `FakeAi::scripted`, uno principal y uno de respaldo,
dados de alta por la API (`/api/ai/profile`, `/api/ai/fallback`).

### tests/test_cli.py (7)

| Python | Rust |
| --- | --- |
| `test_status_scan_and_search` | no se pasa: la línea de órdenes de Python (`danplay/cli.py`) se va con Python |
| `test_a_missing_required_value_is_reported_and_exits` (8 órdenes) | no se pasa: la línea de órdenes de Python |
| `test_folders_and_exclusions` | no se pasa: la línea de órdenes de Python |
| `test_playlists_by_id_or_by_name` | no se pasa: la línea de órdenes de Python |
| `test_duplicates_with_a_folder_outside_the_library` | no se pasa: la línea de órdenes de Python |
| `test_import_convert_and_organize_without_touching_anything` | no se pasa: la línea de órdenes de Python |
| `test_youtube_without_network_says_so` | no se pasa: la línea de órdenes de Python |

### tests/test_no_console.py (4)

| Python | Rust |
| --- | --- |
| `test_the_flag_is_added_unless_a_console_is_requested` | no se pasa: en Rust `CREATE_NO_WINDOW` se pone siempre al lanzar cada programa (`library/src/processes.rs`: `order`), y solo se ve en Windows |
| `test_on_windows_every_process_runs_without_a_window` | no se pasa: envolvía el `Popen` de Python (también el de yt-dlp). En Rust, lo mismo que la fila de arriba |
| `test_on_linux_nothing_is_touched` | no se pasa: el `Popen` de Python. En Rust el indicador solo existe al compilar para Windows |
| `test_importing_danplay_changes_real_popen_only_on_windows` | no se pasa: el `Popen` de Python, cambiado al importar `danplay` |

### tests/test_system_libs.py (4)

| Python | Rust |
| --- | --- |
| `test_the_original_library_path_comes_back` | no se pasa: `LD_LIBRARY_PATH` era del núcleo empaquetado con PyInstaller |
| `test_without_one_before_it_goes_away` | no se pasa: el núcleo empaquetado con PyInstaller |
| `test_outside_the_package_or_linux_nothing_changes` | no se pasa: el núcleo empaquetado con PyInstaller |
| `test_a_real_ffmpeg_child_gets_the_system_libraries` | no se pasa: el núcleo empaquetado con PyInstaller |

`tests/conftest.py`, `tests/api_core.py` y `tests/rust_netns.py` no tienen
pruebas: son la maquinaria de las de Python (el aislamiento, el corte de red,
el núcleo de Rust en su red propia) y se van con ellas. Su biblioteca
sintética está ahora en `testkit.rs`.
