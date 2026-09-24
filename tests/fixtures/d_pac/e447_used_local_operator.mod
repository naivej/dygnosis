// inventory: d_pac_e447_used_local_operator
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  #forecast = var_expectation(nope);
  [name='Y'] y = b*y(-1)+e+forecast;
  [name='Z'] z = z(-1)+e2;
end;
