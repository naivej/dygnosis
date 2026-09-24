// inventory: d_pac_e451_unknown_pac
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  [name='Y'] y = b*y(-1)+e+pac_expectation(nope);
  [name='Z'] z = z(-1)+e2;
end;
