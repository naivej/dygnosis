// inventory: d_surgery_e337_excluded_twice
var c k dummy1;
varexo x;
parameters aa bet;
aa = 1;
bet = 0.99;
model;
[name='e1'] c = -k + aa*x*k(-1);
[name='e2'] c = bet*k + dummy1;
[name='e3'] k = aa*x + dummy1;
end;
shocks;
var x = 0.01;
end;
initval;
k = 1;
end;
model_remove('e1','e2');
