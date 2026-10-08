exports.loop = function () {
  const room = Game.rooms[Memory.room];
  const emitters = room.find(FIND_NURSERY_EMITTERS);
  const sparks = room.find(FIND_NURSERY_SPARKS);
  if (emitters.length !== 1 || !(emitters[0] instanceof NurseryEmitter))
    throw new Error('emitter API missing');
  for (const spark of sparks) {
    if (
      !(spark instanceof NurserySpark) ||
      room
        .lookForAt(LOOK_NURSERY_SPARKS, spark.pos.x, spark.pos.y)
        .every((object) => object.id !== spark.id)
    ) {
      throw new Error('spark class or spatial lookup missing');
    }
    if (Game.getObjectById(spark.id).age !== spark.age) throw new Error('spark lookup differs');
  }
  Memory.loops = (Memory.loops || 0) + 1;
  Memory.emitted = emitters[0].emitted;
  Memory.sparks = sparks.length;
  Memory.ages = sparks.map((spark) => spark.age);
};
