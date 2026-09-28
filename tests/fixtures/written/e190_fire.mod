// inventory: written_e190_fire — two-variable partial-information expectation
var y;
varexo e;
model;
y=EXPECTATION(0)(y+e);
end;
stoch_simul(partial_information) y;
