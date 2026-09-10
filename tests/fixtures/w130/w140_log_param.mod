// inventory: w140_log_param
var x;
varexo e;
parameters betta;
betta = 0.99;
model(linear);
x = log(betta) + x(+1);
end;
