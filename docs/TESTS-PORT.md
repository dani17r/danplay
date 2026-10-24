# Las pruebas de Python, pasadas a Rust

La fase 5 quitó Python del todo ([MIGRATION.md](MIGRATION.md)). Antes de
borrarlo, sus 944 pruebas (359 de la parte del servidor A, 137 de la B y 448
de la biblioteca) pasaron a Rust, con las mismas comprobaciones y los mismos
valores esperados. Ya no hay Python que ejecutar: los archivos de Python que
se nombran aquí (`tests/*.py`, `danplay/`) se fueron en a0e9317 y quedan en
el historial (`git show a0e9317^:tests/test_api.py`). Aquí, prueba a
prueba, dónde está cada una ahora:

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
| `test_errors_come_out_in_spanish` | ya estaba en `jobs.rs`: `tests::failed_jobs_say_why` (el previsto tal cual, el otro con «failure: motivo», sin resultado) |
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
| `test_study_mode_is_saved_in_the_index_and_in_the_file` | `api_tests/songs.rs`: `the_study_mode_is_saved_in_the_index_and_in_the_file` (`routes/songs/tests.rs`: `study_is_validated_and_saved_clean` ya miraba la validación y el vacío) |
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
| `test_a_long_live_stream_is_rejected` | desde la fase 6, ninguna: el tope de duración lo pone yt-dlp (`--match-filter duration<=?1800`, que mira `services/ytdlp/tests.rs`: `the_options_as_arguments`) |
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
| `test_the_options_reach_yt_dlp` | ya estaba en `services/ytdlp/tests.rs`: `the_options_as_arguments` (`--js-runtimes node:/usr/bin/node`) |
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

## La biblioteca por dentro

Las que probaban la biblioteca por dentro: lo que ahora vive en `library`,
`analysis` y `engine` (y, cuando lo que hacía Python lo hace ahora un
servicio del servidor, en `server/src/services/`). De las 448 pruebas de
estos 22 archivos (una con `parametrize` cuenta una vez: en Rust van
los mismos casos en una tabla): 406 nuevas en Rust, 38 que ya estaban
(algunas con lo que les faltaba añadido, dicho en su fila) y 4 que no se
pasan (solo tenían sentido en Python: el puente de PyO3, el SDK de openai,
los tipos de Python, el `pyproject`). Trozos sueltos que no se pasan, cada
uno en su fila: que mutagen lea lo que escribe lofty (mutagen hacía de
testigo), `fingerprint.AVAILABLE`, el «mapeado sin copiar» de numpy y el
`None` de `search_form`.

Una diferencia real, arreglada en Rust: `safe::datum` dejaba pasar algunos
caracteres de formato (abajo, en test_safe.py).

Convenciones de estas pruebas:

- Audio de verdad, con ffmpeg en una carpeta temporal propia que se borra al
  acabar (`library/src/testkit.rs`, `analysis/src/testkit.rs`,
  `server/src/testkit.rs`); sin ffmpeg, la prueba lo dice y vuelve sin
  comprobar nada. Lo mismo con ONNX Runtime para el grafo del separador.
- Nada de Python: los programas de mentira son guiones de sh.
- Lo que en Python se comparaba con una biblioteca de Python
  (python-dotenv, mutagen, `posixpath`/`ntpath`/`json`, el db.py de antes…)
  se compara con lo que daba: casos generados una vez y guardados junto a la
  prueba (`dotenv_cases.json`, `compat_cases.json`, `python_cases.json`,
  `tags/mutagen/`), o la misma cuenta escrita en la prueba.
- Lo que Hypothesis sacaba al azar sale de un generador fijo
  (`testkit::Gen`), con lo que más rompe mezclado.

### tests/test_core.py (85)

| Python | Rust |
| --- | --- |
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

### tests/test_config_rust.py (3)

En Python se comparaba la configuración de Rust con la de danplay/config.py
y con python-dotenv. Sin Python, se compara con lo que daban ellas.

| Python | Rust |
| --- | --- |
| `test_rust_config_matches_the_python_config` | `library/src/config/tests.rs` `config_reads_the_environment_and_the_settings_file` (en otro proceso con ese entorno, como en Python; los valores, los que daba config.py) |
| `test_settings_file_is_read_like_python_dotenv` | `library/src/config/tests.rs` `settings_file_is_read_like_python_dotenv` (400 textos hechos con las mismas líneas, y lo que leía python-dotenv 1.2.4, generados una vez en `library/src/config/dotenv_cases.json`) |
| `test_saved_values_are_written_like_python_and_read_back` | `library/src/config/tests.rs` `saved_values_are_written_like_python_and_read_back` (los ejemplos fijos con lo que escribía config.py, y 2000 textos de un generador fijo en vez de Hypothesis: se escriben como en config.py y se releen iguales) |

### tests/test_resolve.py (61)

El emparejador. Lo que decide cada canción (`Index::matching`) va en
`library/src/assistant/matching/tests/library_cases.rs` (abreviado
`L` aquí abajo), contra una base temporal con las mismas filas a mano; el
lote, el índice por revisión y su cerrojo, en
`server/src/services/chat/resolve/tests.rs` (abreviado `S`). Lo que en
Python era Hypothesis o `random`, con un generador fijo.

| Python | Rust |
| --- | --- |
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

### tests/test_ai.py (61)

En Python el «proveedor» era un cliente del SDK de OpenAI de mentira; en
Rust es un servidor HTTP en el propio equipo que contesta según lo que le
piden (`serve_fn`, y `nobody_home` para uno apagado). Abreviaturas:
`P` = `library/src/ai/providers/tests.rs`, `K` = `library/src/ai/catalog.rs`,
`C` = `library/src/ai/call/tests.rs`, `E` = `library/src/enrich/tests.rs`,
`T` = `library/src/assistant/tools/tests.rs`,
`A` = `server/src/services/chat/tests/ai_cases.rs` (un turno entero por
`/api/chat` y las herramientas con su validación).

| Python | Rust |
| --- | --- |
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
| `test_bundled_snapshot_exists_and_knows_the_big_ones` | `K` `bundled_snapshot_exists_and_knows_the_big_ones` (la foto de `data/models-snapshot.json`) |
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
| `test_invalid_arguments_are_reported_in_spanish` | `A` `invalid_arguments_are_reported_in_spanish` (parte ya estaba en `server/src/services/chat/tests.rs` `tool_arguments_are_checked_and_converted`) |
| `test_numbers_as_text_and_year_as_number_are_accepted` | `A` `numbers_as_text_and_year_as_number_are_accepted` |
| `test_old_names_still_work` | `A` `old_names_still_work` (con un mp3 de verdad: sin archivo, poner estrellas falla), `server/src/services/chat/tools.rs` `old_names_become_the_new_ones` y `T` `lyrics_without_id_go_by_artist_and_title` (la letra por nombre de verdad va a LRCLIB, que no se prueba sin red) |

### tests/test_ai_status.py (30)

Los errores, servidos por HTTP a este mismo cliente con los cuerpos reales
de cada proveedor (en Python los hacía el constructor del SDK de openai); la
hora, el reloj a mano de las pruebas. `S` =
`library/src/ai/call/tests/status.rs`, `A` =
`server/src/services/chat/tests/ai_cases.rs`.

| Python | Rust |
| --- | --- |
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

### tests/test_playlists_place.py (34)

El cerrojo, el aviso (`touch`) y el sellado de los archivos son del servidor
(`services::playlists`, `services::chat::lists::place`): todo va en
`server/src/services/playlists/tests.rs` (`P`), contra la base de verdad. Lo
que Python apuntaba con un `monkeypatch` de `_stamp_playlists_into_files`
aquí se ve en los archivos: canciones con su mp3 de verdad, y las que se
sellan cambian de fecha (sin ffmpeg, esas pruebas no comprueban nada).

| Python | Rust |
| --- | --- |
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

### tests/test_safe.py (27)

Todo en `library/src/safe.rs` (`F`). Las de Hypothesis, con un generador
fijo de texto (letras de todo Unicode y lo que suele romper).

**Diferencia encontrada y arreglada**: `datum` dejaba pasar caracteres de
formato (Cf) que no estaban en su lista, como U+0600 (marca de número árabe)
o U+070F (marca de abreviatura siria). La prueba de propiedades de Python los
pedía fuera (`assert_clean`), pero Hypothesis no llegó a generarlos; el
generador de Rust sí. Ahora `datum` (y `strip_invisible`) quitan cualquier
carácter de formato.

| Python | Rust |
| --- | --- |
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

### tests/test_intake.py (25)

El lector de listas pegadas: `library/src/assistant/intake/tests.rs` (`I`);
las muestras, copiadas a `library/src/assistant/intake/samples.json` (antes
`tests/fixtures/intake_samples.json`). `last_setlist` es del servidor:
`server/src/services/chat/tests/intake_cases.rs` (`S`). Las de Hypothesis,
con un generador fijo.

| Python | Rust |
| --- | --- |
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

### tests/test_chord_sheets.py (18)

`C` = `library/src/chord_sheets/tests.rs` (las páginas, con un `get` de
mentira: lo que no conoce es «sin red»); `E` = `library/src/enrich/tests.rs`.

| Python | Rust |
| --- | --- |
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

### tests/test_media.py (17)

`K` = `library/src/convert/tests.rs`; `E` = `library/src/enrich/tests.rs`
(con el `Fake` de lo de fuera); `SC` = `server/src/services/convert.rs`, `SE`
= `server/src/services/enrich.rs` y `ST` = `server/src/services/tools.rs`
(sus `mod tests`). Para que la app crea que falta un programa que sí está en
el PATH, las pruebas usan `App::hide_tool` (solo en las pruebas).

| Python | Rust |
| --- | --- |
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

### tests/test_properties.py (15)

En Python, con Hypothesis. proptest no está en el espacio de trabajo, así que
van con un generador fijo (`Gen` de `library/src/testkit.rs`, un xorshift):
`CASES` (3000) casos por propiedad, caracteres de todo Unicode mezclados con
lo que más rompe (`NASTY`: controles, marcas sueltas, invisibles, comillas,
emojis, los caracteres prohibidos en un nombre, los nombres reservados de
Windows…). Las de teoría musical, que son pocas combinaciones, prueban
todas: cada nota de la tabla con cada semitono de -36 a 36, y cada par de
notas. `N` = `library/src/names/tests.rs` (`mod properties`), `T` =
`library/src/theory.rs` y `O` = `library/src/toon.rs` (sus `mod tests`).

| Python | Rust |
| --- | --- |
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

### tests/test_compat.py (13)

> Desde la fase 6 ya no están: las reglas de Python que comparaban se
> quitaron del código ([MIGRATION.md](MIGRATION.md), «La fase 6»). Lo de
> abajo cuenta cómo eran.

En Python, cada regla de `library::py` se comparaba en vivo con la de verdad
(a través de `danplay_core._compat`) sobre casos de Hypothesis. Sin Python,
lo que respondían `posixpath`, `ntpath`, `fnmatch`, `round`, `repr`,
`float`, `int` y `json` se guardó una vez (Python 3.13.5, Unicode 15.1) en
`library/src/py/tests/compat_cases.json`: casos de la misma forma que los de
Hypothesis, unos cientos o miles por regla, con sus `@example`, y los
decimales como sus bits. `C` = `library/src/py/tests/compat.rs`.

| Python | Rust |
| --- | --- |
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

### tests/test_schema.py (10)

`S` = `library/src/index/schema/tests.rs`. Lo que en Python se hacía con
`monkeypatch` de `db.SCHEMA` y `db.MIGRATIONS` va con `prepare_with`, que
recibe el esquema y las migraciones (`prepare` le pasa los de verdad).

| Python | Rust |
| --- | --- |
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

### tests/test_schema_rust.py (3)

Comparaba en vivo el esquema y las migraciones de Rust con los de Python
(danplay/library/db.py). Cómo dejaba Python cada base de partida (las
tablas, índices y triggers, la versión, las filas de cada tabla y lo que
encuentra la búsqueda) se guardó una vez en
`library/src/index/schema/python_cases.json`, y se compara con lo que deja
Rust.

| Python | Rust |
| --- | --- |
| `test_rust_leaves_the_db_like_python` | `S` `rust_leaves_the_db_like_python` (las cuatro bases de partida) |
| `test_an_up_to_date_db_does_not_change` | `S` `an_up_to_date_db_does_not_change` |
| `test_a_db_from_a_newer_danplay_is_left_alone` | `S` `a_db_from_a_newer_danplay_is_left_alone` (la misma que la de test_schema.py, que mira también que no se aplique nada) |

### tests/test_separation.py (12)

`P` = `analysis/src/separate/tests.rs` (sin ffmpeg, con la red de mentira
`Identity`); `J` = `analysis/src/separate/process/tests.rs` (el trabajo
entero, con ffmpeg y canciones de tonos en wav). ONNX Runtime, para el grafo
de verdad, se busca en `DANPLAY_ONNXRUNTIME`/`ORT_DYLIB_PATH`, en lo que deja
la compilación en `target/<perfil>/data` o en `desktop/src-tauri/data`
(`analysis/src/testkit.rs`); sin él, esa prueba lo dice y no comprueba nada.

| Python | Rust |
| --- | --- |
| `test_the_spectrogram_and_its_inverse_give_back_the_signal` | ya estaba en `P` `spectrogram_and_inverse_give_back_the_signal` |
| `test_the_pieces_are_joined_back_into_the_whole_song` | ya estaba en `P` `chunks_join_into_the_whole_song` (los tres largos) |
| `test_a_song_with_an_offset_and_a_loud_mix_comes_back_the_same` | `P` `a_song_with_an_offset_and_a_loud_mix_comes_back_the_same` |
| `test_it_stops_when_cancelled` | ya estaba en `P` `stops_on_cancel_and_a_join_error_is_not_lost` |
| `test_a_failure_while_joining_is_not_swallowed` | ya estaba en `P` `stops_on_cancel_and_a_join_error_is_not_lost` |
| `test_each_track_gets_its_wave_on_a_common_scale` | ya estaba en `P` `each_stem_waveform_shares_one_scale` |
| `test_safetensors_are_read_without_the_library` | `P` `safetensors_are_read_without_the_library` |
| `test_the_real_graph_loads_with_weights_laid_out_like_the_official_ones` | `P` `the_real_graph_loads_with_weights_laid_out_like_the_official_ones` (los dos grafos de `data/separator`, con ONNX Runtime de verdad) |
| `test_what_is_not_in_the_song_is_not_a_track_and_all_add_up_to_the_song` | `J` `what_is_not_in_the_song_is_not_a_track_and_all_add_up_to_the_song` (lo que «contó» va en los `Warning` que recibe quien llama, no en líneas de la salida) |
| `test_the_improvement_keeps_what_was_decided` | `J` `the_improvement_keeps_what_was_decided` |
| `test_a_source_that_only_plays_a_moment_stays` | ya estaba en `P` `a_source_heard_only_briefly_is_kept` |
| `test_the_tracks_can_be_kept_in_opus` | `J` `the_tracks_can_be_kept_in_opus` (si ese ffmpeg sabe hacer Opus) |

### tests/test_names.py (8)

`N` = `library/src/names/tests.rs` (`mod search_form_cases`); `M` =
`library/src/assistant/matching/tests.rs`. Las propiedades, con el
generador fijo de `testkit` en vez de Hypothesis.

| Python | Rust |
| --- | --- |
| `test_the_search_form` | `N` `the_search_form` (los casos de la tabla; el de `None` no existe en Rust, que recibe un `&str`: el vacío sí está). Cuatro ya estaban en `N` `n_with_tilde_is_kept` |
| `test_compound_emojis_leave_no_leftovers` | `N` `compound_emojis_leave_no_leftovers` |
| `test_a_turkish_dotted_i_leaves_no_stray_dot` | `N` `a_turkish_dotted_i_leaves_no_stray_dot` |
| `test_search_tokens_are_the_words_of_the_form` | `M` `search_tokens_are_the_words_of_the_form` (`search_tokens` de Python es `toks` de la búsqueda) |
| `test_the_second_pass_folds_only_the_enye` | `M` `the_second_pass_folds_only_the_enye` (`fold_enye` es `fold`) |
| `test_is_idempotent` | `N` `is_idempotent` |
| `test_only_lowercase_letters_digits_and_single_spaces` | `N` `only_lowercase_letters_digits_and_single_spaces` |
| `test_the_enye_is_never_lost` | `N` `the_enye_is_never_lost` |

### tests/test_formats.py (7)

`T` = `library/src/tags/tests.rs`. Los seis formatos (mp3, flac, ogg, opus,
m4a y wav), hechos con ffmpeg; el que ese ffmpeg no sepa escribir se salta.

| Python | Rust |
| --- | --- |
| `test_all_our_own_fields_come_back_when_read` | `T` `all_our_own_fields_come_back_when_read` (casi todo ya estaba en `T` `what_is_written_reads_back_in_every_format`; faltaban el álbum, el género, las etiquetas libres, la portada difuminada y la letra de dos líneas) |
| `test_zero_stars_read_back_as_zero` | `T` `zero_stars_read_back_as_zero_in_every_format` (la del mp3 ya estaba en `T` `zero_stars_read_as_zero`) |
| `test_the_duration_does_not_come_out_zero` | `T` `the_duration_does_not_come_out_zero` (las de antes, `wav_duration_is_not_zero` y `mp3_duration_still_right`, solo miraban que pasara de un segundo) |
| `test_without_tags_the_basics_are_read` | `T` `without_tags_the_basics_are_read` |
| `test_a_format_without_tags_says_so_without_breaking` | ya estaba en `T` `non_audio_reads_empty` |
| `test_timed_lyrics_are_cleaned` | ya estaba en `library/src/tags/lrc.rs` `takes_the_stamps_and_headers_out` |
| `test_with_the_db_lost_study_comes_back_from_the_file` | `server/src/services/library.rs` `with_the_db_lost_study_comes_back_from_the_file` (flac, ogg y opus, con la app: escanear, guardar el estudio, borrar el índice y volver a escanear) |

### tests/test_tags_lofty.py (6)

Comparaba lofty con mutagen, que hacía de testigo. Sin Python, mutagen no
puede leer en vivo lo que escribe lofty; lo que sí se guardó (con mutagen
1.48.1, una vez) son archivos etiquetados por él y lo que leía de ellos:
`library/src/tags/mutagen/` (un archivo por formato con todos los campos y
la portada, uno con veces escuchada, y `expected.json`). `L` =
`library/src/tags/tests.rs` (`mod mutagen_cases`).

| Python | Rust |
| --- | --- |
| `test_what_mutagen_wrote_reads_the_same` | `L` `what_mutagen_wrote_reads_the_same` |
| `test_what_lofty_writes_mutagen_reads` | `L` `what_lofty_writes_reads_like_what_mutagen_wrote` (lo que escribe lofty se lee igual que leía mutagen lo suyo, con el bpm al par, las estrellas y la lista). No se pasa que lo lea mutagen: es una biblioteca de Python |
| `test_lofty_touches_up_mutagen_tags_without_duplicates` | `L` `lofty_touches_up_mutagen_tags_without_duplicates` (sobre una copia del de mutagen; lo de que no se repita nada se mira con lofty: ID3v2.3, un POPM, un APIC, un TXXX:LISTAS, un solo bloque «id3 » en el wav, una imagen en el flac) |
| `test_an_odd_frame_does_not_leave_the_file_unread` | `L` `an_odd_frame_does_not_leave_the_file_unread` (lo de cómo se escribe el año ya estaba en `T` `a_year_that_is_not_a_date_is_kept`; lo que leía mutagen, no se pasa) |
| `test_popm_play_counts_are_kept` | `L` `popm_play_counts_are_kept` (desde el POPM que escribió mutagen) |
| `test_many_at_once_give_the_same_as_one_by_one` | `library/src/index/scan.rs` `many_at_once_give_the_same_as_one_by_one` (`tags.read_many` es ahora la lectura por adelantado del escaneo, `ReadAhead`) |

### tests/test_reuse.py (6)

`R` = `server/src/services/reuse/tests.rs` (con la app y archivos de
verdad).

| Python | Rust |
| --- | --- |
| `test_the_offset_between_two_files_to_the_millisecond` | ya estaba en `analysis/src/align.rs` `the_offset_to_the_millisecond` (los mismos desfases: 0, 0,3217, -1,25 y 4 s; y otra canción no se parece) |
| `test_the_study_moves_with_the_offset` | `library/src/reuse.rs` `the_study_moves_with_the_offset` |
| `test_moving_the_music_and_downloading_again_keeps_everything` | ya estaba en `R` `moving_the_music_and_downloading_again_keeps_everything` |
| `test_a_second_copy_takes_the_click_and_the_study_but_not_another_song` | ya estaba en `R` `a_second_copy_takes_the_click_and_the_study_but_not_another_song` |
| `test_asking_to_separate_a_copy_takes_the_stems_already_made` | ya estaba en `R` `asking_to_separate_a_copy_takes_the_stems_already_made` |
| `test_what_was_lost_before_comes_back_with_one_pass` | ya estaba en `R` `what_was_lost_before_comes_back_with_one_pass` |

### tests/test_mega.py (3)

`G` = `analysis/src/mega/tests.rs`.

| Python | Rust |
| --- | --- |
| `test_the_spectrogram_goes_and_comes_back` | ya estaba en `G` `spectrogram_round_trip` |
| `test_the_split_adds_up_and_follows_what_the_net_hears` | ya estaba en `G` `split_adds_up_and_follows_what_the_net_hears` |
| `test_the_max_pass_keeps_what_sounds_and_the_rest_is_other` | `G` `the_max_pass_keeps_what_sounds_and_the_rest_is_other` (con ffmpeg; lo que «contó» va en los `Warning`) |

### tests/test_click.py (2)

| Python | Rust |
| --- | --- |
| `test_every_sound_is_the_one_of_the_study` | ya estaba en `engine/src/click.rs` `every_sound_matches_the_one_in_the_saved_mix` (los doce sonidos con sus números: el clic de la mezcla es ya el del motor) |
| `test_the_click_track_puts_a_hit_on_every_beat` | `analysis/src/click.rs` `the_click_track_puts_a_hit_on_every_beat` (lo de los pulsos y el tope del volumen ya estaba, a medias, en `each_hit_on_its_beat_and_volume_capped`) |

### tests/test_torchzip.py (2)

Los dos checkpoints de `tests/fixtures` se copiaron a
`analysis/src/weights/fixtures/` (las pruebas de Rust los leían de
`../tests/fixtures`, que se va con Python). `W` =
`analysis/src/weights/tests.rs`.

| Python | Rust |
| --- | --- |
| `test_the_weights_come_out_as_torch_saved_them` | ya estaba en `W` `weights_come_out_as_torch_saved_them` (los nombres, las formas, los tipos y que `c` es una vista de `a`); los números de cada peso, en `W` `the_numbers_come_out_as_torch_saved_them`. Lo de «mapeado, sin copiar» (`flags.writeable` de numpy) no se pasa: Rust lee los números a su vector |
| `test_a_checkpoint_that_runs_code_is_refused` | ya estaba en `W` `checkpoint_that_runs_code_is_refused` |
