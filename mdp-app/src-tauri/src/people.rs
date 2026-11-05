//! Sprint 3 screens: people, pets, families, relationships, places and
//! events. Each command is a thin call into
//! `mdp-core`, which holds the rules and the tests.

use mdp_core::dates::{self, DateAsKnown};
use mdp_core::people::{FamilyView, IndividualView, RelationshipView, GENDERS, PARENT_QUALIFIERS, RELATIONSHIP_KINDS};
use mdp_core::places::{EventView, PlaceView, PlacesAndEvents, EVENT_KINDS};
use mdp_core::summary::Entry;
use mdp_core::Row;
use serde::Serialize;
use tauri::State;

use crate::{with_journey, Res, Shared};

/// The fixed lists the screens offer.
#[derive(Serialize)]
pub struct Choices {
    relationship_kinds: Vec<&'static str>,
    parent_qualifiers: Vec<&'static str>,
    genders: Vec<&'static str>,
    event_kinds: Vec<&'static str>,
}

#[tauri::command]
pub fn choices() -> Choices {
    Choices {
        relationship_kinds: RELATIONSHIP_KINDS.to_vec(),
        parent_qualifiers: PARENT_QUALIFIERS.to_vec(),
        genders: GENDERS.to_vec(),
        event_kinds: EVENT_KINDS.to_vec(),
    }
}

/// Show back how a typed date was understood, while it is typed.
#[tauri::command]
pub fn read_date(text: String) -> Res<Option<DateAsKnown>> {
    dates::parse(&text).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn item_table(state: State<Shared>, id: String) -> Res<String> {
    with_journey(&state, |j| j.item_table(&id))
}

#[tauri::command]
pub fn add_individual(state: State<Shared>, kind: String, name: String) -> Res<String> {
    with_journey(&state, |j| j.add_individual(&kind, &name))
}

#[tauri::command]
pub fn individual_view(state: State<Shared>, id: String) -> Res<IndividualView> {
    with_journey(&state, |j| j.individual_view(&id))
}

#[tauri::command]
pub fn update_individual(state: State<Shared>, id: String, changes: Row) -> Res<()> {
    with_journey(&state, |j| j.update_individual(&id, &changes))
}

#[tauri::command]
pub fn add_relationship(
    state: State<Shared>,
    from: String,
    kind: String,
    to: String,
    qualifier: Option<String>,
    certainty: Option<String>,
    source_note: Option<String>,
) -> Res<String> {
    with_journey(&state, |j| {
        j.add_relationship(&from, &kind, &to, qualifier.as_deref(), certainty.as_deref(), source_note.as_deref())
    })
}

#[tauri::command]
pub fn relationships_of(state: State<Shared>, id: String) -> Res<Vec<RelationshipView>> {
    with_journey(&state, |j| j.relationships_of(&id))
}

#[tauri::command]
pub fn update_relationship(state: State<Shared>, id: String, changes: Row) -> Res<()> {
    with_journey(&state, |j| j.update_relationship(&id, &changes))
}

#[tauri::command]
pub fn remove_relationship(state: State<Shared>, id: String) -> Res<()> {
    with_journey(&state, |j| j.remove_relationship(&id))
}

#[tauri::command]
pub fn add_family(state: State<Shared>, name: String) -> Res<String> {
    with_journey(&state, |j| j.add_family(&name))
}

#[tauri::command]
pub fn family_view(state: State<Shared>, id: String) -> Res<FamilyView> {
    with_journey(&state, |j| j.family_view(&id))
}

#[tauri::command]
pub fn update_family(state: State<Shared>, id: String, changes: Row) -> Res<()> {
    with_journey(&state, |j| j.update_family(&id, &changes))
}

#[tauri::command]
pub fn add_family_member(state: State<Shared>, family: String, individual: String) -> Res<()> {
    with_journey(&state, |j| j.add_family_member(&family, &individual))
}

#[tauri::command]
pub fn remove_family_member(state: State<Shared>, family: String, individual: String) -> Res<()> {
    with_journey(&state, |j| j.remove_family_member(&family, &individual))
}

#[tauri::command]
pub fn relate_families(state: State<Shared>, a: String, b: String) -> Res<()> {
    with_journey(&state, |j| j.relate_families(&a, &b))
}

#[tauri::command]
pub fn unrelate_families(state: State<Shared>, a: String, b: String) -> Res<()> {
    with_journey(&state, |j| j.unrelate_families(&a, &b))
}

#[tauri::command]
pub fn places_and_events(state: State<Shared>) -> Res<PlacesAndEvents> {
    with_journey(&state, |j| j.places_and_events())
}

#[tauri::command]
pub fn add_place(state: State<Shared>, name: String) -> Res<String> {
    with_journey(&state, |j| j.add_place(&name))
}

#[tauri::command]
pub fn place_view(state: State<Shared>, id: String) -> Res<PlaceView> {
    with_journey(&state, |j| j.place_view(&id))
}

#[tauri::command]
pub fn update_place(state: State<Shared>, id: String, changes: Row) -> Res<()> {
    with_journey(&state, |j| j.update_place(&id, &changes))
}

/// Sprint 10: relationships the data supports, and the answer to one.
#[tauri::command]
pub fn suggested_relationships(state: State<Shared>, id: String) -> Res<Vec<mdp_core::sorting::SuggestedRelationship>> {
    with_journey(&state, |j| j.suggested_relationships(&id))
}

#[tauri::command]
pub fn answer_suggestion(state: State<Shared>, suggestion: mdp_core::sorting::SuggestedRelationship, accept: bool) -> Res<String> {
    with_journey(&state, |j| j.answer_suggestion(&suggestion, accept))
}

/// Sprint 10: merge two entries for the same person or pet, keeping the old ID as an alias.
#[tauri::command]
pub fn merge_individuals(state: State<Shared>, keep: String, other: String) -> Res<()> {
    with_journey(&state, |j| j.merge_individuals(&keep, &other))
}

#[tauri::command]
pub fn merge_places(state: State<Shared>, keep: String, other: String) -> Res<()> {
    with_journey(&state, |j| j.merge_places(&keep, &other))
}

#[tauri::command]
pub fn add_event(state: State<Shared>, kind: String, name: Option<String>, people: Vec<String>, when: String, place: Option<String>) -> Res<String> {
    with_journey(&state, |j| j.add_event(&kind, name.as_deref(), &people, &when, place.as_deref()))
}

/// This Journey's kinds of event: the fixed ones and the family's own (sprint 19).
#[tauri::command]
pub fn event_kinds(state: State<Shared>) -> Res<Vec<mdp_core::places::EventKind>> {
    with_journey(&state, |j| j.event_kinds())
}

#[tauri::command]
pub fn event_view(state: State<Shared>, id: String) -> Res<EventView> {
    with_journey(&state, |j| j.event_view(&id))
}

#[tauri::command]
pub fn update_event(state: State<Shared>, id: String, changes: Row) -> Res<()> {
    with_journey(&state, |j| j.update_event(&id, &changes))
}

#[tauri::command]
pub fn set_event_people(state: State<Shared>, id: String, people: Vec<String>) -> Res<()> {
    with_journey(&state, |j| j.set_event_people(&id, &people))
}

#[tauri::command]
pub fn removal_warning(state: State<Shared>, id: String) -> Res<Vec<String>> {
    with_journey(&state, |j| j.removal_warning(&id))
}

#[tauri::command]
pub fn remove_item(state: State<Shared>, id: String) -> Res<()> {
    with_journey(&state, |j| j.remove_item(&id))
}

/// Sprint 8: portraits for people and pets, and adding several people to a family at once.
#[tauri::command]
pub fn portraits(state: State<Shared>) -> Res<std::collections::HashMap<String, String>> {
    with_journey(&state, |j| j.portraits())
}

#[tauri::command]
pub fn portrait_choices(state: State<Shared>, id: String) -> Res<Vec<Entry>> {
    with_journey(&state, |j| j.portrait_choices(&id))
}

#[tauri::command]
pub fn set_portrait(state: State<Shared>, id: String, picture: Option<String>) -> Res<()> {
    with_journey(&state, |j| j.set_portrait(&id, picture.as_deref().filter(|p| !p.is_empty())))
}

#[tauri::command]
pub fn add_family_members(state: State<Shared>, family: String, individuals: Vec<String>) -> Res<usize> {
    with_journey(&state, |j| j.add_family_members(&family, &individuals))
}
