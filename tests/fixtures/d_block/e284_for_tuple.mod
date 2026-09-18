// inventory: e284_for_tuple
var y;
varexo e;
parameters rho;
rho = 0.9;
@#for (a, b) in [(1, 2, 3)]
@#define z = a
@#endfor
model;
y = rho * y(-1) + e;
end;
