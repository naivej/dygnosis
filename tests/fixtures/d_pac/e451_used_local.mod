// inventory: d_pac_e451_used_local
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  # pterm = pac_expectation(nope);
  [name='Y'] y = b*y(-1)+e+pterm;
  [name='Z'] z = z(-1)+e2;
end;
