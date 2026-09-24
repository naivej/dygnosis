// inventory: d_pac_quiet_unused_local_pac
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  # pterm = pac_expectation(nope);
  [name='Y'] y = b*y(-1)+e+y(-1);
  [name='Z'] z = z(-1)+e2;
end;
