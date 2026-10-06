//! 旧用例的线程独立夹具装配器；每次搜索显式借用不可变快照。
#![allow(dead_code)]
use kk_pathfinder::*;
use std::{cell::RefCell, collections::BTreeMap, sync::Arc};
type Fixture = (BTreeMap<String, [u8; 625]>, Arc<TerrainSet>);
thread_local! { static FIXTURE: RefCell<Fixture> = RefCell::new((BTreeMap::new(), Arc::new(TerrainSet::default()))); }
pub fn load_terrain(name: &str, bits: &[u8; 625]) -> Result<(), Error> {
    parse_room_name(name).ok_or_else(|| Error::InvalidRoomName(name.into()))?;
    FIXTURE.with(|fixture| {
        let mut fixture = fixture.borrow_mut();
        fixture.0.insert(name.into(), *bits);
        fixture.1 = Arc::new(TerrainSet::new(
            fixture.0.iter().map(|(name, bits)| (name.clone(), *bits)),
        )?);
        Ok(())
    })
}
fn snapshot() -> Arc<TerrainSet> {
    FIXTURE.with(|f| f.borrow().1.clone())
}
pub fn search(
    origin: WorldPosition,
    goals: &[Goal],
    options: &SearchOptions,
) -> Result<SearchResult, Error> {
    kk_pathfinder::search(&snapshot(), origin, goals, options)
}
pub fn search_with_room_callback<'a, F: FnMut(u8, u8) -> RoomDecision<'a>>(
    origin: WorldPosition,
    goals: &[Goal],
    options: &SearchOptions,
    callback: F,
) -> Result<SearchResult, Error> {
    kk_pathfinder::search_with_room_callback(&snapshot(), origin, goals, options, callback)
}
pub fn search_with_host<H: SearchHost>(
    origin: WorldPosition,
    goals: &[Goal],
    options: &SearchOptions,
    host: &mut H,
) -> Result<SearchResult, Error> {
    kk_pathfinder::search_with_host(&snapshot(), origin, goals, options, host)
}
