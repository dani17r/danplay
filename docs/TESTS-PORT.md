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
|---|---|
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
