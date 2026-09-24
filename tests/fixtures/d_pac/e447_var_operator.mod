// inventory: d_pac_e447_var_operator
var y z;
varexo e e2;
parameters b;
b = 0.8;
model;
  [name='Y'] y = b*y(-1)+e+var_expectation(nope);
  [name='Z'] z = z(-1)+e2;
end;
