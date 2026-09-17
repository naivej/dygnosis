// inventory: e218_histval_quiet
var y;
varexo e;
parameters rho;
rho = 0.9;
model;
y = rho * y(-1) + e;
end;
histval(all_values_required);
y(0) = 0;
end;
