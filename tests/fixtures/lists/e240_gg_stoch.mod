// inventory: lists_e240_gg_stoch
// The same epilogue name on the shipped `stoch_simul` surface.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

epilogue;
gg = y + c;
end;

stoch_simul gg;
