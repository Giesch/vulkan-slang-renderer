coll.ToThings(a) : where [coll.to_things : coll -> Thing(a)]

Thing(a) :: b

map : input -> Thing(a) where [input.ToThings(a)]
map = |input| input.to_things()
