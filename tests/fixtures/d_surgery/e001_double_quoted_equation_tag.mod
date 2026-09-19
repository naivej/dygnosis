// inventory: d_surgery_e001_double_quoted_equation_tag
var c k dummy1;
varexo x;
parameters aa bet;
aa = 1;
bet = 0.99;
model;
[name="e1"] c = -k + aa*x*k(-1);
[name='e2'] dummy1 = bet*k;
[name='e3'] k = aa*x + dummy1;
end;
shocks;
var x = 0.01;
end;
initval;
k = 1;
end;