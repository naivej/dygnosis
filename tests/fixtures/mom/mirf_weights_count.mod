// inventory: mom_e391_weights_count
// Three weights against two periods. One weight would have been broadcast.
var y c;
varexo e;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

matched_irfs;
var y; varexo e; periods 1 2; values 1 2; weights 3 4 5;
end;
