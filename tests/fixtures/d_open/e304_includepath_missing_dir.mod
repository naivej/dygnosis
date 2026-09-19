// inventory: e304_includepath_missing_dir
@#includepath "missing_dir"
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
