// inventory: lists_e239_forecast_z
// `forecast` names a symbol the file never declares. 7.1 refuses at check.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
y = a*y(-1) + e;
c = y;
end;

forecast z;
