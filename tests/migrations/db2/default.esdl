module default {
    type Bar { link spam -> Spam };
    type Spam { link bar -> Bar };
};
