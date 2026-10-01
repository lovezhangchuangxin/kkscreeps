// 使用路径调用宏，且不把宏或 Schema trait 导入作用域。
kk_schema::kk_schema! {
    pub Payload {
        (pub marker: u16)
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub name: str)
    }
}

mod tags {
    pub const PAYLOAD: u8 = 42;
}

kk_schema::kk_room_object! {
    pub enum Object {
        Payload(Payload) = tags::PAYLOAD,
    }
}

#[test]
fn exported_macros_work_without_caller_imports() {
    let mut object = Object::Payload(Payload {
        marker: 123,
        id: 1,
        x: 2,
        y: 3,
        name: "矿物".into(),
    });
    object.set_id(9);
    object.set_pos(4, 5);
    assert_eq!(object.id(), 9);
    assert_eq!(object.pos(), (4, 5));
    assert_eq!(Payload::OFFSET_x, 10);

    let mut bytes = Vec::new();
    kk_schema::Schema::encode_into(&object, &mut bytes);
    assert_eq!(bytes[0], tags::PAYLOAD);
    let mut cursor = kk_schema::Cursor::new(&bytes);
    let back = <Object as kk_schema::Schema>::decode_from(&mut cursor).unwrap();
    assert_eq!(back, object);
    assert_eq!(cursor.remaining(), 0);
}
