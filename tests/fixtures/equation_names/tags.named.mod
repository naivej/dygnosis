var y;
model;
    # x = 1;
    [static]
    y = 1;
    [name='eq_1']
    y = y(-1);
    [dynamic, group='g', name='eq_2']
    y = y(-1);
end;
