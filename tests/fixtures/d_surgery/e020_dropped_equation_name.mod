// inventory: d_surgery_e020_dropped_equation_name
var c k;
varexo x;
parameters aa bet;
aa = 1;
bet = 0.99;
model;
[name='e1'] zzz = aa*x;
[name='e2'] k = aa*x*k(-1);
end;
shocks;
var x = 0.01;
end;
model_remove('e1');
