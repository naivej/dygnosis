// inventory: written_e190_quiet — single-variable expectation
var y;
model;
y=EXPECTATION(0)(y);
end;
stoch_simul(partial_information) y;
