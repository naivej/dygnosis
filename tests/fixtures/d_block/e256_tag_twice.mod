// inventory: e256_tag_twice
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
[name='a', name='b'] y = rho * y(-1) + e;
end;
