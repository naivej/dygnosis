// inventory: e218_all_values
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
initval(all_values_required);
y = 0;
end;
