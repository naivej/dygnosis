// inventory: w140_mul_param
var x;
varexo e;
parameters betta;
betta = 0.99;
model(linear);
x = betta * x;
end;
