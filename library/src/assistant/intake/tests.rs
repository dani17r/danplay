use super::*;

#[test]
fn a_whatsapp_setlist() {
    let text = "*Domingo*\n1. *Digno De Adorar* ✅ @Ana Torres Voz https://youtu.be/AbCdEfGhIjK\n2. Rey Infinito (En Su Presencia)\n";
    let p = parse_setlist(text);
    assert!(p.is_setlist, "{p:?}");
    assert_eq!(p.items.len(), 2);
    assert_eq!(p.items[0].title, "Digno De Adorar");
    assert_eq!(
        p.items[0].url,
        "https://www.youtube.com/watch?v=AbCdEfGhIjK"
    );
    assert_eq!(p.items[0].people, vec!["Ana Torres".to_owned()]);
    assert_eq!(p.items[1].title, "Rey Infinito");
    assert_eq!(p.items[1].note, "En Su Presencia");
}

#[test]
fn attached_pronouns() {
    assert_eq!(verb_base("agregalas", Some(&ORDER_VERBS)), "agrega");
    assert_eq!(verb_base("metela", Some(&ORDER_VERBS)), "mete");
    assert_eq!(verb_base("bajala", None), "baja");
}

#[test]
fn reads_the_intent() {
    let i = read_intent("no las bajes, solo verifica");
    assert!(i.no_download && i.check);
    assert!(read_intent("crea una lista nueva").create);
}

#[test]
fn youtube_links() {
    assert_eq!(
        links::clean_youtube_url("youtu.be/AbCdEfGhIjK?si=x"),
        "https://www.youtube.com/watch?v=AbCdEfGhIjK"
    );
    assert_eq!(
        links::clean_youtube_url("https://youtube.com.evil.example/watch?v=AbCdEfGhIjK"),
        ""
    );
    assert_eq!(
        links::clean_youtube_url("https://www.youtube.com/playlist?list=PLx1"),
        "https://www.youtube.com/playlist?list=PLx1"
    );
}
