// @ts-check
// Si el recorrido del modo estudio ya se vio. Aparte de useStudyTour.js
// para que el estudio pueda preguntarlo sin cargar el recorrido: ese trae
// driver.js con su CSS (unos 28 KB), y solo sale una vez en la vida.

export const TOUR_KEY = 'danplay.study.tourSeen'

/** Si ya se vio (o se cerró) alguna vez. */
export function tourSeen() {
  try {
    return localStorage.getItem(TOUR_KEY) === 'true'
  } catch {
    return true
  }
}
