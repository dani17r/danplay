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
