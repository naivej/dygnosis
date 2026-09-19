// inventory: d_surgery_quiet_remove_many
var c k dummy1;
varexo x;
parameters aa bet;
aa = 1;
bet = 0.99;
model;
[grp='g'] c = -k + aa*x*k(-1);
[grp='g'] dummy1 = k;
[name='e3'] k = aa*x + bet*dummy1;
end;
shocks;
var x = 0.01;
end;
initval;
k = 1;
end;
model_remove([grp='g']);
